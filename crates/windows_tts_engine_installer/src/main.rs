//! Installer for text-to-speech COM Server.
//!
//! # References.
//!
//! - <https://github.com/gexgd0419/NaturalVoiceSAPIAdapter/blob/master/Installer/Install.cpp>

use std::{ffi::OsStr, path::Path};

use anyhow::{bail, Context};
use clap::Parser;
use windows::{
    core::{w, Free, PCWSTR},
    Win32::{
        Foundation::MAX_PATH,
        System::{
            LibraryLoader::GetModuleFileNameW,
            Registry::{
                RegCreateKeyExW, RegDeleteKeyExW, RegSetValueExW, HKEY_CURRENT_USER, KEY_SET_VALUE,
                KEY_WOW64_64KEY, REG_SZ,
            },
        },
    },
};

pub fn to_utf16(s: impl AsRef<OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    s.as_ref()
        .encode_wide()
        .chain(core::iter::once(0u16))
        .collect()
}

const DLL_NAMES: &[&str] = &["windows_tts_engine.dll", "windows_tts_engine_piper.dll"];

const UNINSTALL_REG_KEY: PCWSTR =
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Lej77WindowsTextToSpeechEngine");
const UNINSTALL_ARGS: &str = " --uninstall";

/// Register uninstaller with Windows so the user can easily uninstall the
/// text-to-speech engine.
///
/// # References
///
/// - Adapted from:
///   <https://github.com/gexgd0419/NaturalVoiceSAPIAdapter/blob/2573a979a71ee96d3370676dd6f6acb382e4d35e/Installer/Install.cpp#L38-L60>
fn add_uninstall_registry_key() -> anyhow::Result<()> {
    // Gather info:
    let mut uninstall_cmd_line = [0_u16; MAX_PATH as usize + UNINSTALL_ARGS.len()];
    char::encode_utf16('"', &mut uninstall_cmd_line[..1]);

    let mut len =
        unsafe { GetModuleFileNameW(None, &mut uninstall_cmd_line[1..MAX_PATH as usize + 1]) };
    if len == 0 || len == MAX_PATH {
        return Err(windows::core::Error::from_win32().into());
    }

    char::encode_utf16('"', &mut uninstall_cmd_line[len as usize + 1..][..1]);
    len += 2; // for quotes

    assert_ne!(
        uninstall_cmd_line[len as usize - 1],
        0,
        "last string character should not be a nul terminator\n\t\
            string: {}",
        String::from_utf16_lossy(&uninstall_cmd_line[..len as usize]),
    );
    assert_eq!(
        uninstall_cmd_line[len as usize], 0,
        "nul terminator should be at end of string"
    );

    uninstall_cmd_line[len as usize..][..UNINSTALL_ARGS.len() + /* nul byte: */ 1]
        .copy_from_slice(&to_utf16(UNINSTALL_ARGS));

    let version = to_utf16(clap::crate_version!());
    let authors = to_utf16(clap::crate_authors!());
    let info_to_write = [
        (w!("DisplayName"), w!("windows_tts_engine")),
        (w!("DisplayVersion"), PCWSTR::from_raw(version.as_ptr())),
        (w!("Publisher"), PCWSTR::from_raw(authors.as_ptr())),
        (
            w!("UninstallString"),
            PCWSTR::from_raw(uninstall_cmd_line.as_ptr()),
        ),
        (
            w!("HelpLink"),
            w!("https://github.com/Lej77/windows-text-to-speech/issues"),
        ),
        (
            w!("URLInfoAbout"),
            w!("https://github.com/Lej77/windows-text-to-speech/"),
        ),
        (
            w!("URLUpdateInfo"),
            w!("https://github.com/Lej77/windows-text-to-speech/releases"),
        ),
    ];

    // Update the Windows registry:

    let mut key = Default::default();
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            UNINSTALL_REG_KEY,
            None,
            None,
            Default::default(),
            KEY_SET_VALUE | KEY_WOW64_64KEY,
            None,
            &mut key,
            None,
        )
    }
    .ok()
    .context("Failed to create uninstall registry key")?;

    for (name, data) in info_to_write {
        unsafe { RegSetValueExW(key, name, None, REG_SZ, Some(data.as_wide().align_to().1)) }
            .ok()
            .with_context(|| {
                format!(
                    "Failed to set registry value for key \"{}\"",
                    unsafe { name.to_string() }.unwrap_or_else(|_| "unknown".to_owned())
                )
            })?;
    }

    unsafe { key.free() };

    Ok(())
}

fn remove_uninstall_registry_key() -> anyhow::Result<()> {
    unsafe {
        RegDeleteKeyExW(
            HKEY_CURRENT_USER,
            UNINSTALL_REG_KEY,
            KEY_WOW64_64KEY.0,
            None,
        )
    }
    .ok()
    .context("Failed to remove uninstall registry key")?;
    Ok(())
}

/// Adapted from
/// <https://github.com/gexgd0419/NaturalVoiceSAPIAdapter/blob/2573a979a71ee96d3370676dd6f6acb382e4d35e/Installer/Install.cpp#L67-L109>
fn register(dll_path: &Path, regsvr_popups: bool) -> anyhow::Result<()> {
    let mut command = runas::Command::new("regsvr32");
    if !regsvr_popups {
        command.arg("/s"); // silent
    }
    let status = command
        .arg(dll_path)
        .status()
        .context("Failed to start regsvr32 to register the COM server")?;
    if !status.success() {
        bail!(
            "regsvr32 completed unsuccessfully{}",
            status
                .code()
                .map(|code| format!(" (Exit code: {code})"))
                .unwrap_or_default()
        );
    }
    Ok(())
}

/// Adapted from
/// <https://github.com/gexgd0419/NaturalVoiceSAPIAdapter/blob/2573a979a71ee96d3370676dd6f6acb382e4d35e/Installer/Install.cpp#L111-L131>
fn unregister(dll_path: &Path, regsvr_popups: bool) -> anyhow::Result<()> {
    let mut command = runas::Command::new("regsvr32");
    command.arg("/u");
    if !regsvr_popups {
        command.arg("/s"); // silent
    }

    let status = command
        .arg(dll_path)
        .status()
        .context("Failed to start regsvr32 to unregister the COM server")?;
    if !status.success() {
        bail!(
            "regsvr32 completed unsuccessfully{}",
            status
                .code()
                .map(|code| format!(" (Exit code: {code})"))
                .unwrap_or_default()
        );
    }
    Ok(())
}

/// Installer for text-to-speech engine.
#[derive(Parser)]
struct Args {
    /// Uninstall the text-to-speech engine.
    #[clap(long)]
    uninstall: bool,
    /// Show message box popups with result information from "regsvr32".
    #[clap(long)]
    regsvr_popups: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let exe_path =
        std::env::current_exe().context("Failed to get location of current executable")?;
    let exe_dir = exe_path
        .parent()
        .context("Failed to get directory of current executable")?;

    let mut first = true;

    for dll_name in DLL_NAMES {
        let dll_path = exe_dir.join(dll_name);
        if !dll_path.exists() {
            eprintln!("Could not find DLL at:\n\t{}", dll_path.display());
            eprintln!(
                "Ensure the installer program is in the same folder as the \
                text-to-speech engine DLL you want to install.\n"
            );
            continue;
        }

        let was_first = std::mem::replace(&mut first, false);

        if args.uninstall {
            unregister(&dll_path, args.regsvr_popups)?;
        } else {
            if was_first {
                // Add uninstaller before registering anything.
                add_uninstall_registry_key()?;
            }
            register(&dll_path, args.regsvr_popups)?;
        }
    }

    if first {
        eprintln!("No text-to-speech engine DLL could be found, uninstallation failed!\n");
        std::process::exit(2);
    }

    if args.uninstall {
        // Remove uninstaller only when we know we have succeeded:
        remove_uninstall_registry_key()?;
    }

    Ok(())
}
