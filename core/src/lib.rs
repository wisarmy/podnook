//! 剪贴板历史的领域模型、捕获规则与本地数据库。
//!
//! 这个 crate 不依赖 NSH / AppKit：可以在普通 `cargo test` 里快速跑完，
//! 应用层只负责把 `NSPasteboard` 的快照喂进来。

// M1 先把数据层 API 铺齐，界面在 M2 才消费；测试已覆盖这些入口。
#![allow(dead_code)]

pub mod db;

use std::time::{SystemTime, UNIX_EPOCH};

/// 文本条目存储上限（超出则截断存储，剪贴板上仍是完整内容）。
pub const TEXT_LIMIT: usize = 100 * 1024;

/// 图片条目上限：编码后超过就整条跳过，不写半条脏数据。
pub const IMAGE_LIMIT: usize = 10 * 1024 * 1024;

/// 默认保留条数（pin 的条目不计入、也不被清理）。
pub const RETENTION_COUNT: i64 = 1000;

/// 默认保留天数。
pub const RETENTION_DAYS: i64 = 30;

/// 轮询间隔（毫秒）。
pub const POLL_MS: u64 = 300;

/// `settings` 表里的键。
pub const SETTING_RETENTION_COUNT: &str = "retention.count";
pub const SETTING_RETENTION_DAYS: &str = "retention.days";
pub const SETTING_IGNORED_BUNDLES: &str = "ignored.bundles";
pub const SETTING_HOTKEY: &str = "hotkey";
pub const SETTING_AUTO_PASTE: &str = "auto_paste";
pub const SETTING_LAUNCH_AT_LOGIN: &str = "launch_at_login";

/// 默认忽略的来源 App：密码管理器/钥匙串，避免敏感内容进历史。
pub const DEFAULT_IGNORED_BUNDLES: &[&str] = &["com.apple.Passwords"];

/// 条目的类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipKind {
    Text,
    Link,
    Image,
    File,
}

impl ClipKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Link => "link",
            Self::Image => "image",
            Self::File => "file",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "text" => Some(Self::Text),
            "link" => Some(Self::Link),
            "image" => Some(Self::Image),
            "file" => Some(Self::File),
            _ => None,
        }
    }
}

/// 一次剪贴板快照（macOS 侧读到的东西）。
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub text: Option<String>,
    pub image_png: Option<Vec<u8>>,
    /// 图片像素尺寸 `(宽, 高)`。
    pub image_size: Option<(u32, u32)>,
    /// 文件条目的路径（已从 file URL 解出）。
    pub file_uris: Vec<String>,
    /// 文件总字节数，未知为 `None`。
    pub file_bytes: Option<i64>,
    pub source_app: Option<String>,
    pub source_bundle: Option<String>,
}

impl Snapshot {
    /// 有内容的类型判定：文件 > 图片 > 文本/链接。
    pub fn kind(&self) -> Option<ClipKind> {
        if !self.file_uris.is_empty() {
            return Some(ClipKind::File);
        }
        if self.image_png.is_some() {
            return Some(ClipKind::Image);
        }
        if let Some(text) = &self.text {
            if !text.trim().is_empty() {
                return Some(kind_for_text(text));
            }
        }
        None
    }
}

/// 准备写入数据库的一条新记录。
#[derive(Debug, Clone)]
pub struct NewClip {
    pub kind: ClipKind,
    /// 文本/链接正文（可能已截断）。
    pub content: Option<String>,
    pub preview: String,
    /// 内容指纹（基于未截断的完整内容）。
    pub hash: String,
    pub image_path: Option<String>,
    pub image_w: Option<i64>,
    pub image_h: Option<i64>,
    pub image_bytes: Option<i64>,
    pub file_uris: Vec<String>,
    pub source_app: Option<String>,
    pub source_bundle: Option<String>,
    pub byte_size: i64,
    pub truncated: bool,
}

/// 数据库里的一行。
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub id: i64,
    pub kind: ClipKind,
    pub content: Option<String>,
    pub preview: String,
    pub hash: String,
    pub image_path: Option<String>,
    pub image_w: Option<i64>,
    pub image_h: Option<i64>,
    pub image_bytes: Option<i64>,
    pub file_uris: Vec<String>,
    pub source_app: Option<String>,
    pub source_bundle: Option<String>,
    pub byte_size: i64,
    pub truncated: bool,
    pub created_at: i64,
    pub last_copied_at: i64,
    pub pinned: bool,
    pub copy_count: i64,
}

/// 当前 Unix 时间（秒）。
pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 文本是不是一个单独的 http(s) 链接。
pub fn is_url(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() || t.split_whitespace().count() != 1 {
        return false;
    }
    if !(t.starts_with("http://") || t.starts_with("https://")) {
        return false;
    }
    !t.chars().any(|c| c.is_control())
}

/// 文本的类型：链接或普通文本。
pub fn kind_for_text(text: &str) -> ClipKind {
    if is_url(text) {
        ClipKind::Link
    } else {
        ClipKind::Text
    }
}

/// 按字节上限截断，且落在字符边界上。返回 `(截断后的文本, 是否截断)`。
pub fn truncate_text(text: &str, limit: usize) -> (String, bool) {
    if text.len() <= limit {
        return (text.to_string(), false);
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_string(), true)
}

/// 列表里的单行预览：第一段非空行，最多 80 个字符。
pub fn preview_for_text(text: &str) -> String {
    let line = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    let mut chars = line.chars();
    let head: String = chars.by_ref().take(80).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

/// 内容指纹（未截断的完整内容）。
pub fn hash_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// 文本内容的指纹。
pub fn hash_text(text: &str) -> String {
    hash_bytes(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_links() {
        assert_eq!(kind_for_text("https://nsh.dev/"), ClipKind::Link);
        assert_eq!(kind_for_text("  https://a.b/c  "), ClipKind::Link);
        assert_eq!(kind_for_text("看这个 https://nsh.dev/"), ClipKind::Text);
        assert_eq!(kind_for_text("ftp://nsh.dev/"), ClipKind::Text);
        assert_eq!(kind_for_text("astryx"), ClipKind::Text);
        assert_eq!(
            kind_for_text("https://a.b/\nhttps://c.d/"),
            ClipKind::Text,
            "多行不算链接"
        );
    }

    #[test]
    fn snapshot_priority_is_file_then_image_then_text() {
        let mut snap = Snapshot {
            text: Some("hello".into()),
            ..Default::default()
        };
        assert_eq!(snap.kind(), Some(ClipKind::Text));

        snap.image_png = Some(vec![1, 2, 3]);
        assert_eq!(snap.kind(), Some(ClipKind::Image));

        snap.file_uris = vec!["/tmp/a.txt".into()];
        assert_eq!(snap.kind(), Some(ClipKind::File));

        let empty = Snapshot::default();
        assert_eq!(empty.kind(), None);
        let blank = Snapshot {
            text: Some("   \n".into()),
            ..Default::default()
        };
        assert_eq!(blank.kind(), None);
    }

    #[test]
    fn truncates_on_char_boundary() {
        let text = "中文内容".repeat(10_000);
        let (cut, truncated) = truncate_text(&text, 7);
        assert!(truncated);
        assert_eq!(cut, "中文");
        assert_eq!(cut.len(), 6);
        let (same, truncated) = truncate_text("short", 100);
        assert!(!truncated);
        assert_eq!(same, "short");
    }

    #[test]
    fn preview_is_first_line_capped() {
        assert_eq!(preview_for_text("\n\n  第一行  \n第二行"), "第一行");
        let long = "字".repeat(200);
        let p = preview_for_text(&long);
        assert_eq!(p.chars().count(), 81); // 80 + 省略号
        assert!(p.ends_with('…'));
    }

    #[test]
    fn hashes_are_stable() {
        assert_eq!(hash_text("abc"), hash_text("abc"));
        assert_ne!(hash_text("abc"), hash_text("abd"));
        assert_eq!(hash_text("abc").len(), 64);
    }
}
