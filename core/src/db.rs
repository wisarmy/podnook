//! 本地历史库：`rusqlite`（SQLite）+ 图片文件缓存。
//!
//! 一个 `Store` 同时服务轮询线程与界面线程：连接放在 `Mutex` 里，
//! 每次操作短持有，不在锁内做文件 IO 之外的重活。

// M1 先把数据层 API 铺齐，界面在 M2 才消费；测试已覆盖这些入口。
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use crate::{
    hash_bytes, hash_text, kind_for_text, preview_for_text, truncate_text, Clip, ClipKind, NewClip,
    Snippet, Snapshot, IMAGE_LIMIT, RETENTION_COUNT, RETENTION_DAYS, SETTING_IGNORED_BUNDLES,
    SETTING_RETENTION_COUNT, SETTING_RETENTION_DAYS, TEXT_LIMIT,
};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS clips (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  kind           TEXT    NOT NULL,
  content        TEXT,
  preview        TEXT    NOT NULL DEFAULT '',
  hash           TEXT    NOT NULL UNIQUE,
  image_path     TEXT,
  image_w        INTEGER,
  image_h        INTEGER,
  image_bytes    INTEGER,
  file_uris      TEXT,
  source_app     TEXT,
  source_bundle  TEXT,
  byte_size      INTEGER NOT NULL DEFAULT 0,
  truncated      INTEGER NOT NULL DEFAULT 0,
  created_at     INTEGER NOT NULL,
  last_copied_at INTEGER NOT NULL,
  copy_count     INTEGER NOT NULL DEFAULT 0,
  pinned         INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS clips_order ON clips(pinned DESC, last_copied_at DESC);
CREATE INDEX IF NOT EXISTS clips_age ON clips(last_copied_at);
CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS snippets (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  name         TEXT    NOT NULL,
  content      TEXT    NOT NULL,
  keyword      TEXT    NOT NULL DEFAULT '',
  pinned       INTEGER NOT NULL DEFAULT 0,
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL,
  last_used_at INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS snippets_used ON snippets(last_used_at DESC, created_at DESC);
"#;

/// 写入结果：新条目，还是把已有条目置顶。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Upsert {
    Inserted,
    Bumped,
}

/// 一次捕获的结果。
#[derive(Debug, Clone)]
pub struct Capture {
    pub kind: ClipKind,
    pub hash: String,
    pub upsert: Upsert,
}

pub struct Store {
    conn: Mutex<Connection>,
    /// 数据目录：`history.db` 与 `images/` 都在这里。
    dir: PathBuf,
    /// 下一次轮询要忽略的内容指纹（我们自己写回剪贴板的那一条）。
    self_write: Mutex<Option<String>>,
}

/// 旧库升级：补上 `copy_count` 列并把已有条目计为复制过一次；清掉
/// 早期版本写进 `preview` 的界面文案（图片/文件条目的预览改为按类型渲染）。
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(clips)")?;
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);
    if !columns.iter().any(|name| name == "copy_count") {
        conn.execute(
            "ALTER TABLE clips ADD COLUMN copy_count INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
        conn.execute("UPDATE clips SET copy_count = 1", [])?;
    }
    conn.execute(
        "UPDATE clips SET preview = '' WHERE kind IN ('image', 'file') AND preview <> ''",
        [],
    )?;
    let mut stmt = conn.prepare("PRAGMA table_info(snippets)")?;
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);
    if !columns.is_empty() && !columns.iter().any(|name| name == "pinned") {
        conn.execute(
            "ALTER TABLE snippets ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    Ok(())
}

impl Store {
    /// 打开（或创建）`dir` 下的历史库。
    pub fn open(dir: &Path) -> rusqlite::Result<Self> {
        std::fs::create_dir_all(dir.join("images")).map_err(|e| {
            rusqlite::Error::InvalidParameterName(format!("create data dir: {e}"))
        })?;
        let conn = Connection::open(dir.join("history.db"))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            dir: dir.to_path_buf(),
            self_write: Mutex::new(None),
        })
    }

    /// 测试用：内存库。
    pub fn open_in_memory(dir: PathBuf) -> rusqlite::Result<Self> {
        // 图片条目会把 PNG 写到 dir/images；内存库也要有目录。
        std::fs::create_dir_all(dir.join("images")).map_err(|e| {
            rusqlite::Error::InvalidParameterName(format!("create data dir: {e}"))
        })?;
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            dir,
            self_write: Mutex::new(None),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 标记我们即将写回剪贴板的内容，下一次轮询读到它时跳过。
    pub fn mark_self_write(&self, hash: String) {
        *self.self_write.lock().unwrap_or_else(|e| e.into_inner()) = Some(hash);
    }

    /// 把一条剪贴板快照落库。没有可记录内容、超限或属于自身写入时返回 `None`。
    pub fn capture(&self, snapshot: Snapshot, now: i64) -> rusqlite::Result<Option<Capture>> {
        let Some(kind) = snapshot.kind() else {
            return Ok(None);
        };

        // 用户选择忽略的来源 App，不记录。
        if let Some(bundle) = &snapshot.source_bundle {
            if self.ignored_bundles()?.iter().any(|b| b == bundle) {
                log::debug!("skip ignored app {bundle}");
                return Ok(None);
            }
        }

        let (hash, byte_size) = match kind {
            ClipKind::Text | ClipKind::Link => {
                let text = snapshot.text.clone().unwrap_or_default();
                (hash_text(&text), text.len() as i64)
            }
            ClipKind::Image => {
                let png = snapshot.image_png.as_deref().unwrap_or_default();
                if png.len() > IMAGE_LIMIT {
                    log::info!(
                        "skip image: {} bytes over the {} limit",
                        png.len(),
                        IMAGE_LIMIT
                    );
                    return Ok(None);
                }
                (hash_bytes(png), png.len() as i64)
            }
            ClipKind::File => {
                let joined = snapshot.file_uris.join("\n");
                (hash_bytes(joined.as_bytes()), snapshot.file_bytes.unwrap_or(0))
            }
        };

        {
            let mut self_write = self.self_write.lock().unwrap_or_else(|e| e.into_inner());
            if self_write.as_deref() == Some(hash.as_str()) {
                *self_write = None;
                log::debug!("skip our own clipboard write");
                return Ok(None);
            }
        }

        let (content, preview, truncated) = match kind {
            ClipKind::Text | ClipKind::Link => {
                let text = snapshot.text.clone().unwrap_or_default();
                let (stored, truncated) = truncate_text(&text, TEXT_LIMIT);
                let preview = preview_for_text(&stored);
                (Some(stored), preview, truncated)
            }
            ClipKind::Image => (None, String::new(), false),
            ClipKind::File => (None, String::new(), false),
        };

        let mut image_path = None;
        if kind == ClipKind::Image {
            let bytes = snapshot.image_png.as_deref().unwrap_or_default();
            let path = self.dir.join("images").join(format!("{hash}.png"));
            if let Err(e) = std::fs::write(&path, bytes) {
                log::error!("write image {}: {e}", path.display());
                return Ok(None);
            }
            image_path = Some(path.to_string_lossy().to_string());
        }

        let (image_w, image_h) = match snapshot.image_size {
            Some((w, h)) => (Some(w as i64), Some(h as i64)),
            None => (None, None),
        };

        let clip = NewClip {
            kind,
            content,
            preview,
            hash: hash.clone(),
            image_path,
            image_w,
            image_h,
            image_bytes: if kind == ClipKind::Image {
                Some(byte_size)
            } else {
                None
            },
            file_uris: snapshot.file_uris.clone(),
            source_app: snapshot.source_app.clone(),
            source_bundle: snapshot.source_bundle.clone(),
            byte_size,
            truncated,
        };

        let upsert = self.upsert(&clip, now)?;
        let (count, days) = self.retention()?;
        let _ = self.prune_with(now, count, days);
        log::info!(
            "captured {} ({} bytes, {:?})",
            clip.kind.as_str(),
            clip.byte_size,
            upsert
        );
        Ok(Some(Capture {
            kind,
            hash,
            upsert,
        }))
    }

    /// 写入一条记录；同 hash 已存在时只更新「最近复制时间」并置顶。
    pub fn upsert(&self, clip: &NewClip, now: i64) -> rusqlite::Result<Upsert> {
        let conn = self.lock();
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM clips WHERE hash = ?1",
                params![clip.hash],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(id) = existing {
            conn.execute(
                "UPDATE clips SET
                   last_copied_at = ?1,
                   copy_count = copy_count + 1,
                   source_app = COALESCE(?2, source_app),
                   source_bundle = COALESCE(?3, source_bundle)
                 WHERE id = ?4",
                params![now, clip.source_app, clip.source_bundle, id],
            )?;
            return Ok(Upsert::Bumped);
        }

        let file_uris = serde_json::to_string(&clip.file_uris).unwrap_or_else(|_| "[]".into());
        conn.execute(
            "INSERT INTO clips
               (kind, content, preview, hash, image_path, image_w, image_h, image_bytes,
                file_uris, source_app, source_bundle, byte_size, truncated,
                created_at, last_copied_at, copy_count, pinned)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14, 1, 0)",
            params![
                clip.kind.as_str(),
                clip.content,
                clip.preview,
                clip.hash,
                clip.image_path,
                clip.image_w,
                clip.image_h,
                clip.image_bytes,
                file_uris,
                clip.source_app,
                clip.source_bundle,
                clip.byte_size,
                clip.truncated as i64,
                now,
            ],
        )?;
        Ok(Upsert::Inserted)
    }

    /// 按「pin 优先、最近复制优先」列出条目。
    pub fn list(&self, limit: i64) -> rusqlite::Result<Vec<Clip>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, kind, content, preview, hash, image_path, image_w, image_h,
                    image_bytes, file_uris, source_app, source_bundle, byte_size,
                    truncated, created_at, last_copied_at, pinned, copy_count
             FROM clips
             ORDER BY pinned DESC, last_copied_at DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            let kind: String = row.get(1)?;
            let file_uris: Option<String> = row.get(9)?;
            Ok(Clip {
                id: row.get(0)?,
                kind: ClipKind::parse(&kind).unwrap_or(ClipKind::Text),
                content: row.get(2)?,
                preview: row.get(3)?,
                hash: row.get(4)?,
                image_path: row.get(5)?,
                image_w: row.get(6)?,
                image_h: row.get(7)?,
                image_bytes: row.get(8)?,
                file_uris: file_uris
                    .and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok())
                    .unwrap_or_default(),
                source_app: row.get(10)?,
                source_bundle: row.get(11)?,
                byte_size: row.get(12)?,
                truncated: row.get::<_, i64>(13)? != 0,
                created_at: row.get(14)?,
                last_copied_at: row.get(15)?,
                pinned: row.get::<_, i64>(16)? != 0,
                copy_count: row.get(17)?,
            })
        })?;
        rows.collect()
    }

    /// 粘贴/复制后把条目置顶（不增加复制次数；次数按捕获计）。
    pub fn touch_usage(&self, id: i64, now: i64) -> rusqlite::Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE clips SET last_copied_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    /// 修正图片条目的尺寸（迁移旧的像素尺寸到逻辑点数）。
    pub fn set_image_dimensions(&self, id: i64, width: u32, height: u32) -> rusqlite::Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE clips SET image_w = ?1, image_h = ?2 WHERE id = ?3",
            params![width as i64, height as i64, id],
        )?;
        Ok(())
    }

    /// 设置/取消 pin。
    pub fn set_pinned(&self, id: i64, pinned: bool) -> rusqlite::Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE clips SET pinned = ?1 WHERE id = ?2",
            params![pinned as i64, id],
        )?;
        Ok(())
    }

    /// 删除一条，并清掉它的图片文件。
    pub fn delete(&self, id: i64) -> rusqlite::Result<()> {
        let conn = self.lock();
        let image_path: Option<Option<String>> = conn
            .query_row(
                "SELECT image_path FROM clips WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        conn.execute("DELETE FROM clips WHERE id = ?1", params![id])?;
        drop(conn);
        if let Some(Some(path)) = image_path {
            let _ = std::fs::remove_file(path);
        }
        Ok(())
    }

    /// 读一条设置。
    pub fn get_setting(&self, key: &str) -> rusqlite::Result<Option<String>> {
        let conn = self.lock();
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
    }

    /// 写一条设置。
    pub fn set_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    fn retention(&self) -> rusqlite::Result<(i64, i64)> {
        let count = self
            .get_setting(SETTING_RETENTION_COUNT)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(RETENTION_COUNT);
        let days = self
            .get_setting(SETTING_RETENTION_DAYS)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(RETENTION_DAYS);
        Ok((count, days))
    }

    /// 被忽略来源 App 的 bundle id 列表；没设置过时返回默认的密码管理器列表。
    pub fn ignored_bundles(&self) -> rusqlite::Result<Vec<String>> {
        match self.get_setting(SETTING_IGNORED_BUNDLES)? {
            Some(json) => Ok(serde_json::from_str::<Vec<String>>(&json).unwrap_or_default()),
            None => Ok(crate::DEFAULT_IGNORED_BUNDLES
                .iter()
                .map(|bundle| (*bundle).to_string())
                .collect()),
        }
    }

    /// 编辑文本/链接条目的内容，重新计算类型、预览、哈希与大小。
    pub fn update_text(&self, id: i64, text: &str) -> rusqlite::Result<()> {
        if text.trim().is_empty() {
            return Err(rusqlite::Error::InvalidParameterName(
                "clipboard content cannot be empty".into(),
            ));
        }
        let (stored, truncated) = truncate_text(text, TEXT_LIMIT);
        let kind = kind_for_text(&stored);
        let preview = preview_for_text(&stored);
        let hash = hash_text(&stored);
        let byte_size = stored.len() as i64;
        let conn = self.lock();
        conn.execute(
            "UPDATE clips
             SET kind = ?1, content = ?2, preview = ?3, hash = ?4,
                 byte_size = ?5, truncated = ?6
             WHERE id = ?7",
            params![
                kind.as_str(),
                stored,
                preview,
                hash,
                byte_size,
                truncated as i64,
                id
            ],
        )?;
        Ok(())
    }

    /// 删除 `cutoff` 之后复制的条目（pin 的保留）并删除图片文件。
    pub fn delete_since(&self, cutoff: i64) -> rusqlite::Result<usize> {
        self.remove_matching(
            "SELECT id, image_path FROM clips
             WHERE pinned = 0 AND last_copied_at >= ?1",
            params![cutoff],
        )
    }

    /// 清空全部历史（pin 的也删掉）并删除图片文件。
    pub fn clear_history(&self) -> rusqlite::Result<usize> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT image_path FROM clips")?;
        let images: Vec<Option<String>> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        drop(stmt);
        let count = conn.execute("DELETE FROM clips", [])?;
        drop(conn);
        for path in images.into_iter().flatten() {
            let _ = std::fs::remove_file(path);
        }
        Ok(count)
    }

    /// 清理过期条目：超过 `days` 天或超出最近 `count` 条（pin 的永不清理）。
    pub fn prune(&self, now: i64) -> rusqlite::Result<usize> {
        self.prune_with(now, RETENTION_COUNT, RETENTION_DAYS)
    }

    pub fn prune_with(&self, now: i64, count: i64, days: i64) -> rusqlite::Result<usize> {
        let cutoff = now - days * 86_400;
        let mut deleted = 0;
        deleted += self.remove_matching(
            "SELECT id, image_path FROM clips
             WHERE pinned = 0 AND last_copied_at < ?1",
            params![cutoff],
        )?;
        deleted += self.remove_matching(
            "SELECT id, image_path FROM clips
             WHERE pinned = 0 AND id NOT IN (
               SELECT id FROM clips WHERE pinned = 0
               ORDER BY last_copied_at DESC LIMIT ?1
             )",
            params![count],
        )?;
        Ok(deleted)
    }

    fn remove_matching(
        &self,
        sql: &str,
        args: impl rusqlite::Params,
    ) -> rusqlite::Result<usize> {
        let conn = self.lock();
        let mut stmt = conn.prepare(sql)?;
        let rows: Vec<(i64, Option<String>)> = stmt
            .query_map(args, |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        drop(stmt);
        for (id, image_path) in &rows {
            conn.execute("DELETE FROM clips WHERE id = ?1", params![id])?;
            if let Some(path) = image_path {
                let _ = std::fs::remove_file(path);
            }
        }
        Ok(rows.len())
    }
}

impl Store {
    /// 片段库，最近使用的在前。
    pub fn list_snippets(&self) -> rusqlite::Result<Vec<Snippet>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, content, keyword, pinned, created_at, updated_at, last_used_at
               FROM snippets
              ORDER BY pinned DESC, last_used_at DESC, created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Snippet {
                id: row.get(0)?,
                name: row.get(1)?,
                content: row.get(2)?,
                keyword: row.get(3)?,
                pinned: row.get::<_, i64>(4)? != 0,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                last_used_at: row.get(7)?,
            })
        })?;
        rows.collect()
    }

    pub fn create_snippet(
        &self,
        name: &str,
        content: &str,
        keyword: &str,
        now: i64,
    ) -> rusqlite::Result<i64> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO snippets (name, content, keyword, created_at, updated_at, last_used_at)
             VALUES (?1, ?2, ?3, ?4, ?4, 0)",
            params![name, content, keyword, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_snippet(
        &self,
        id: i64,
        name: &str,
        content: &str,
        keyword: &str,
        now: i64,
    ) -> rusqlite::Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE snippets
                SET name = ?1, content = ?2, keyword = ?3, updated_at = ?4
              WHERE id = ?5",
            params![name, content, keyword, now, id],
        )?;
        Ok(())
    }

    pub fn set_snippet_pinned(&self, id: i64, pinned: bool) -> rusqlite::Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE snippets SET pinned = ?1 WHERE id = ?2",
            params![pinned as i64, id],
        )?;
        Ok(())
    }

    pub fn delete_snippet(&self, id: i64) -> rusqlite::Result<()> {
        let conn = self.lock();
        conn.execute("DELETE FROM snippets WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// 粘贴/复制后置顶（按最近使用排序）。
    pub fn touch_snippet(&self, id: i64, now: i64) -> rusqlite::Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE snippets SET last_used_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        let dir = std::env::temp_dir().join(format!("podnook-db-{}", now_secs_nanos()));
        Store::open_in_memory(dir).unwrap()
    }

    fn now_secs_nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    fn text_clip(text: &str) -> Snapshot {
        Snapshot {
            text: Some(text.into()),
            ..Default::default()
        }
    }

    #[test]
    fn inserts_and_dedupes_to_the_top() {
        let store = store();
        let first = store.capture(text_clip("hello"), 100).unwrap().unwrap();
        assert_eq!(first.upsert, Upsert::Inserted);

        let second = store.capture(text_clip("hello"), 200).unwrap().unwrap();
        assert_eq!(second.upsert, Upsert::Bumped);

        let rows = store.list(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].last_copied_at, 200);
        assert_eq!(rows[0].created_at, 100);
        assert_eq!(rows[0].preview, "hello");
        assert_eq!(rows[0].copy_count, 2);
    }

    #[test]
    fn usage_touch_moves_entry_to_top() {
        let store = store();
        store.capture(text_clip("a"), 100).unwrap();
        store.capture(text_clip("b"), 200).unwrap();
        let a = store
            .list(10)
            .unwrap()
            .into_iter()
            .find(|clip| clip.preview == "a")
            .unwrap();
        store.touch_usage(a.id, 300).unwrap();
        let rows = store.list(10).unwrap();
        assert_eq!(rows[0].preview, "a");
        assert_eq!(rows[0].copy_count, 1);
    }

    #[test]
    fn ignored_bundles_default_to_password_managers() {
        let store = store();
        assert!(
            store
                .ignored_bundles()
                .unwrap()
                .iter()
                .any(|bundle| bundle == "com.apple.Passwords")
        );
        store.set_setting(SETTING_IGNORED_BUNDLES, "[]").unwrap();
        assert!(store.ignored_bundles().unwrap().is_empty());
    }

    #[test]
    fn migrates_copy_count_for_existing_databases() {
        let dir = std::env::temp_dir().join(format!("podnook-migrate-{}", now_secs_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        {
            let conn = Connection::open(dir.join("history.db")).unwrap();
            conn.execute_batch(
                "CREATE TABLE clips (
                   id INTEGER PRIMARY KEY AUTOINCREMENT,
                   kind TEXT NOT NULL,
                   content TEXT,
                   preview TEXT NOT NULL DEFAULT '',
                   hash TEXT NOT NULL UNIQUE,
                   image_path TEXT,
                   image_w INTEGER,
                   image_h INTEGER,
                   image_bytes INTEGER,
                   file_uris TEXT,
                   source_app TEXT,
                   source_bundle TEXT,
                   byte_size INTEGER NOT NULL DEFAULT 0,
                   truncated INTEGER NOT NULL DEFAULT 0,
                   created_at INTEGER NOT NULL,
                   last_copied_at INTEGER NOT NULL,
                   pinned INTEGER NOT NULL DEFAULT 0
                 );
                 INSERT INTO clips
                   (kind, content, preview, hash, byte_size, truncated, created_at, last_copied_at, pinned)
                 VALUES ('text', 'hi', 'hi', 'hash', 2, 0, 1, 1, 0);
                 INSERT INTO clips
                   (kind, content, preview, hash, image_w, image_h, byte_size, truncated, created_at, last_copied_at, pinned)
                 VALUES ('image', NULL, '图片 1×1', 'hash2', 1, 1, 3, 0, 1, 2, 0);",
            )
            .unwrap();
        }
        let store = Store::open(&dir).unwrap();
        let rows = store.list(10).unwrap();
        let text = rows.iter().find(|row| row.kind == ClipKind::Text).unwrap();
        assert_eq!(text.copy_count, 1);
        let image = rows.iter().find(|row| row.kind == ClipKind::Image).unwrap();
        assert_eq!(image.preview, "");
        assert_eq!(image.image_w, Some(1));
    }

    #[test]
    fn classifies_links_and_files() {
        let store = store();
        store.capture(text_clip("https://nsh.dev/"), 1).unwrap();
        store
            .capture(
                Snapshot {
                    file_uris: vec!["/tmp/a.txt".into(), "/tmp/b.txt".into()],
                    file_bytes: Some(42),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        let rows = store.list(10).unwrap();
        assert_eq!(rows[0].kind, ClipKind::File);
        assert_eq!(rows[0].preview, "");
        assert_eq!(rows[0].file_uris, ["/tmp/a.txt", "/tmp/b.txt"]);
        assert_eq!(rows[0].byte_size, 42);
        assert_eq!(rows[1].kind, ClipKind::Link);
    }

    #[test]
    fn corrects_image_dimensions() {
        let store = store();
        store
            .capture(
                Snapshot {
                    image_png: Some(vec![1, 2, 3]),
                    image_size: Some((998, 762)),
                    ..Default::default()
                },
                1,
            )
            .unwrap();

        let row = store.list(1).unwrap().remove(0);
        assert_eq!(row.preview, "");
        assert_eq!(row.image_w, Some(998));

        store.set_image_dimensions(row.id, 499, 381).unwrap();
        let row = store.list(1).unwrap().remove(0);
        assert_eq!(row.image_w, Some(499));
        assert_eq!(row.image_h, Some(381));
    }

    #[test]
    fn updates_text_content_and_kind() {
        let store = store();
        store.capture(text_clip("hello"), 1).unwrap();
        let id = store.list(1).unwrap()[0].id;

        store.update_text(id, "https://nsh.dev/").unwrap();
        let row = store.list(1).unwrap().remove(0);
        assert_eq!(row.kind, ClipKind::Link);
        assert_eq!(row.content.as_deref(), Some("https://nsh.dev/"));
        assert_eq!(row.preview, "https://nsh.dev/");
        assert_eq!(row.byte_size, 16);
    }

    #[test]
    fn deletes_recent_entries_but_keeps_pinned() {
        let store = store();
        store.capture(text_clip("old"), 100).unwrap();
        store.capture(text_clip("mid"), 200).unwrap();
        store.capture(text_clip("new"), 300).unwrap();

        let mid = store
            .list(10)
            .unwrap()
            .into_iter()
            .find(|clip| clip.preview == "mid")
            .unwrap();
        store.set_pinned(mid.id, true).unwrap();

        // 150 之后：mid（pin，保留）和 new（删除）。
        assert_eq!(store.delete_since(150).unwrap(), 1);
        let rows = store.list(10).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|clip| clip.preview == "old"));
        assert!(rows.iter().any(|clip| clip.preview == "mid" && clip.pinned));
    }

    #[test]
    fn skips_our_own_write_once() {
        let store = store();
        let hash = hash_text("pasted");
        store.mark_self_write(hash);
        assert!(store.capture(text_clip("pasted"), 1).unwrap().is_none());
        // 第二次是用户自己复制的，应该记录。
        assert!(store.capture(text_clip("pasted"), 2).unwrap().is_some());
    }

    #[test]
    fn truncates_long_text_but_hashes_the_original() {
        let store = store();
        let full = "字".repeat(TEXT_LIMIT);
        store.capture(text_clip(&full), 1).unwrap();
        let rows = store.list(1).unwrap();
        assert!(rows[0].truncated);
        let stored = rows[0].content.as_ref().unwrap();
        assert!(stored.len() <= TEXT_LIMIT);
        // 102400 落在字符中间，截断点会回退到 102399。
        assert_eq!(stored.len(), TEXT_LIMIT - 1);
        assert!(full.starts_with(stored));
        assert_eq!(rows[0].byte_size, full.len() as i64);
    }

    #[test]
    fn prunes_by_age_but_keeps_pins() {
        let store = store();
        store.capture(text_clip("old"), 1).unwrap();
        let old_id = store.list(1).unwrap()[0].id;
        store.set_pinned(old_id, true).unwrap();

        // 新条目写入时 capture 会顺手 prune；pin 的旧条目必须留下。
        store.capture(text_clip("new"), 40 * 86_400).unwrap();
        let rows = store.list(10).unwrap();
        assert_eq!(rows.len(), 2);

        store.set_pinned(old_id, false).unwrap();
        let deleted = store.prune(40 * 86_400).unwrap();
        assert_eq!(deleted, 1);
        let rows = store.list(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].preview, "new");
    }

    #[test]
    fn ignores_apps_from_settings() {
        let store = store();
        store
            .set_setting(crate::SETTING_IGNORED_BUNDLES, r#"["com.apple.Terminal"]"#)
            .unwrap();
        assert_eq!(
            store.ignored_bundles().unwrap(),
            vec!["com.apple.Terminal".to_string()]
        );

        let mut ignored = text_clip("secret");
        ignored.source_bundle = Some("com.apple.Terminal".into());
        assert!(store.capture(ignored, 1).unwrap().is_none());

        let mut kept = text_clip("kept");
        kept.source_bundle = Some("com.apple.Safari".into());
        assert!(store.capture(kept, 2).unwrap().is_some());
    }

    #[test]
    fn retention_settings_apply() {
        let store = store();
        store
            .set_setting(crate::SETTING_RETENTION_COUNT, "2")
            .unwrap();
        store
            .set_setting(crate::SETTING_RETENTION_DAYS, "365")
            .unwrap();
        for i in 0..4 {
            store
                .capture(text_clip(&format!("clip {i}")), i as i64)
                .unwrap();
        }
        assert_eq!(store.list(10).unwrap().len(), 2);
    }

    #[test]
    fn clear_history_removes_everything() {
        let store = store();
        store.capture(text_clip("a"), 1).unwrap();
        store.capture(text_clip("b"), 2).unwrap();
        assert_eq!(store.clear_history().unwrap(), 2);
        assert!(store.list(10).unwrap().is_empty());
    }

    #[test]
    fn prunes_beyond_the_count_limit() {
        let store = store();
        for i in 0..(RETENTION_COUNT + 5) {
            store
                .capture(text_clip(&format!("clip {i}")), i as i64)
                .unwrap();
        }
        // capture 每次都 prune，所以最后一次写入后已经只剩 1000 条。
        let rows = store.list(RETENTION_COUNT + 10).unwrap();
        assert_eq!(rows.len() as i64, RETENTION_COUNT);
        assert_eq!(rows[0].preview, format!("clip {}", RETENTION_COUNT + 4));
        assert_eq!(rows.last().unwrap().preview, "clip 5");
        // 再 prune 一次是幂等的。
        assert_eq!(store.prune(RETENTION_COUNT + 5).unwrap(), 0);
    }
}

#[cfg(test)]
mod snippet_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn store() -> Store {
        let dir = std::env::temp_dir().join(format!(
            "podnook-snippets-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        Store::open_in_memory(dir).unwrap()
    }

    #[test]
    fn snippets_list_recently_used_first() {
        let store = store();
        let first = store.create_snippet("签名", "Best regards", "", 10).unwrap();
        let second = store.create_snippet("地址", "Shanghai", "addr", 20).unwrap();

        let list = store.list_snippets().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, second);
        assert_eq!(list[0].keyword, "addr");

        store.touch_snippet(first, 30).unwrap();
        assert_eq!(store.list_snippets().unwrap()[0].id, first);
    }

    #[test]
    fn snippets_update_and_delete() {
        let store = store();
        let id = store.create_snippet("签名", "Best", "", 10).unwrap();

        store.update_snippet(id, "签名", "Regards", "sig", 40).unwrap();
        let saved = store.list_snippets().unwrap().remove(0);
        assert_eq!(saved.name, "签名");
        assert_eq!(saved.content, "Regards");
        assert_eq!(saved.keyword, "sig");
        assert_eq!(saved.updated_at, 40);

        store.delete_snippet(id).unwrap();
        assert!(store.list_snippets().unwrap().is_empty());
    }
}
