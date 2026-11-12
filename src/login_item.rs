//! 开机自启：`SMAppService.mainApp`（macOS 13+）。
//!
//! 开发用的裸 runner 不是 `.app`，系统会返回 NotFound / 注册失败；
//! 只有打包后的 Podnook.app 才能真正注册。

#[cfg(target_os = "macos")]
use nsh_platform_macos::objc2_foundation::NSError;
#[cfg(target_os = "macos")]
use objc2_service_management::{SMAppService, SMAppServiceStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Enabled,
    Disabled,
    RequiresApproval,
    NotFound,
}

impl Status {
    pub fn is_on(self) -> bool {
        matches!(self, Self::Enabled | Self::RequiresApproval)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Enabled => "已开启",
            Self::Disabled => "未开启",
            Self::RequiresApproval => "等待系统批准",
            Self::NotFound => "需要打包后的 .app",
        }
    }
}

#[cfg(target_os = "macos")]
pub fn status() -> Status {
    unsafe {
        let service = SMAppService::mainAppService();
        let status = service.status();
        if status == SMAppServiceStatus::Enabled {
            Status::Enabled
        } else if status == SMAppServiceStatus::RequiresApproval {
            Status::RequiresApproval
        } else if status == SMAppServiceStatus::NotRegistered {
            Status::Disabled
        } else {
            Status::NotFound
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn status() -> Status {
    Status::NotFound
}

#[cfg(target_os = "macos")]
pub fn set_enabled(enabled: bool) -> Result<Status, String> {
    unsafe {
        let service = SMAppService::mainAppService();
        let current = service.status();
        let result = if enabled {
            if current == SMAppServiceStatus::Enabled {
                Ok(())
            } else {
                service.registerAndReturnError()
            }
        } else if current == SMAppServiceStatus::NotRegistered {
            Ok(())
        } else {
            service.unregisterAndReturnError()
        };
        match result {
            Ok(()) => Ok(status()),
            Err(error) => Err(describe(&error)),
        }
    }
}

#[cfg(target_os = "macos")]
fn describe(error: &NSError) -> String {
    let message = error.localizedDescription().to_string();
    format!("{message}（code {}）", error.code())
}

#[cfg(not(target_os = "macos"))]
pub fn set_enabled(_enabled: bool) -> Result<Status, String> {
    Err("仅支持 macOS".to_string())
}
