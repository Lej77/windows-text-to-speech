use std::{path::PathBuf, sync::OnceLock};

#[cfg(any(feature = "release_logging", debug_assertions))]
use crate::utils::{get_current_dll_path, safe_catch_unwind};

pub struct DllLogger {
    log_path: OnceLock<Option<PathBuf>>,
    init: std::sync::Once,
}
impl DllLogger {
    #[expect(
        clippy::new_without_default,
        reason = "we only want a const constructor"
    )]
    pub const fn new() -> Self {
        Self {
            log_path: OnceLock::new(),
            init: std::sync::Once::new(),
        }
    }
    pub fn write_to_log(&self, _args: core::fmt::Arguments<'_>) {
        #[cfg(any(feature = "release_logging", debug_assertions))]
        safe_catch_unwind::<_, ()>(std::panic::AssertUnwindSafe(|| {
            let Some(log_path) = self.log_path.get_or_init(|| {
                let mut buffer = [0; windows::Win32::Foundation::MAX_PATH as usize];
                Some(
                    std::path::PathBuf::from(
                        String::from_utf16(get_current_dll_path(&mut buffer).ok()?).ok()?,
                    )
                    .with_extension("debug.log"),
                )
            }) else {
                return;
            };

            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_path)
            {
                let _ = std::io::Write::write_all(&mut file, format!("{_args}\n").as_bytes());
            }
        }));
    }
    pub fn install(&'static self) {
        #[cfg(any(feature = "release_logging", debug_assertions))]
        self.init.call_once(|| {
            safe_catch_unwind::<_, ()>(|| {
                if let Err(e) = log::set_logger(self) {
                    self.write_to_log(format_args!("Failed to install logger: {e}"));
                } else {
                    log::set_max_level(log::LevelFilter::Debug);
                    self.write_to_log(format_args!("installed logger"));
                }

                let prev = std::panic::take_hook();
                std::panic::set_hook(Box::new(move |info| {
                    self.write_to_log(format_args!(
                        "-----------\n\
                        {info}\n\
                        ------------"
                    ));
                    prev(info);
                }));
            });
        });
    }
}
impl log::Log for DllLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Debug
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            self.write_to_log(format_args!("{} - {}", record.level(), record.args()));
        }
    }

    fn flush(&self) {}
}
