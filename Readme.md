# Windows text-to-speech

This repository contains a Rust CLI program that uses Windows' text-to-speech APIs to read text passed to the program.

## Usage

[Install Rust](https://www.rust-lang.org/tools/install) and clone this repository, then you can run:

```powershell
cargo run -- This text will be read
```

Alternatively download the `windows_text_to_speech.exe` binary from the [latest release](https://github.com/Lej77/windows-text-to-speech/releases) and run that from the command line:

```powershell
./windows_text_to_speech.exe This text will be read
```

## References

- There are two APIs: [text to speech - Windows 10 TTS voices not showing up? - Stack
  Overflow](https://stackoverflow.com/questions/40406719/windows-10-tts-voices-not-showing-up/40427509#40427509)
- Legacy API: [Text-to-Speech Tutorial (SAPI 5.3) | Microsoft
  Learn](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ms720163(v=vs.85))
- Modern API: [Windows.Media.SpeechSynthesis Namespace - Windows apps | Microsoft
  Learn](https://learn.microsoft.com/en-us/uwp/api/windows.media.speechsynthesis?view=winrt-26100&redirectedfrom=MSDN)
- Detect language: [Microsoft Language Detection - Win32 apps | Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/intl/microsoft-language-detection)
  - [About Extended Linguistic Services - Win32 apps | Microsoft Learn](https://learn.microsoft.com/pl-pl/windows/win32/intl/about-extended-linguistic-services)
  - [Requesting Text Recognition - Win32 apps | Microsoft Learn](https://learn.microsoft.com/pl-pl/windows/win32/intl/requesting-text-recognition)

## License

This project is released under either:

- [MIT License](./LICENSE-MIT)
- [Apache License (Version 2.0)](./LICENSE-APACHE)

at your choosing.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
