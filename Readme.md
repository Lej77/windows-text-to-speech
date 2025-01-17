# Windows text-to-speech

This repository contains a Rust CLI program that uses Windows' text-to-speech APIs to read text passed to the program.

## Usage

[Install Rust](https://www.rust-lang.org/tools/install) and then you can install this program from source:

```powershell
cargo install --git "https://github.com/Lej77/windows-text-to-speech"
# Latest "windows_text_to_speech.exe" will be built and placed inside "%UserProfile%/.cargo/bin/"
windows_text_to_speech.exe This text will be read

cargo uninstall windows_text_to_speech
```

If you have cloned this repository, then you can run the code using:

```powershell
cargo run -- This text will be read
```

Alternatively download the `windows_text_to_speech.exe` binary from the [latest release](https://github.com/Lej77/windows-text-to-speech/releases) and run that from the command line:

```powershell
./windows_text_to_speech.exe This text will be read
```

If you have [Cargo B(inary)Install](https://github.com/cargo-bins/cargo-binstall) then it can download the latest release for you:

```powershell
cargo binstall --git "https://github.com/Lej77/windows-text-to-speech" windows_text_to_speech
# Latest "windows_text_to_speech.exe" will be downloaded to "%UserProfile%/.cargo/bin/"
windows_text_to_speech.exe This text will be read

cargo uninstall windows_text_to_speech
```

## References

Text-to-speech on Windows:

- There are two APIs: [text to speech - Windows 10 TTS voices not showing up? - Stack
  Overflow](https://stackoverflow.com/questions/40406719/windows-10-tts-voices-not-showing-up/40427509#40427509)
- Legacy API: [Text-to-Speech Tutorial (SAPI 5.3) | Microsoft
  Learn](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ms720163(v=vs.85))
- Modern API: [Windows.Media.SpeechSynthesis Namespace - Windows apps | Microsoft
  Learn](https://learn.microsoft.com/en-us/uwp/api/windows.media.speechsynthesis?view=winrt-26100&redirectedfrom=MSDN)
- Detect language: [Microsoft Language Detection - Win32 apps | Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/intl/microsoft-language-detection)
  - [About Extended Linguistic Services - Win32 apps | Microsoft Learn](https://learn.microsoft.com/pl-pl/windows/win32/intl/about-extended-linguistic-services)
  - [Requesting Text Recognition - Win32 apps | Microsoft Learn](https://learn.microsoft.com/pl-pl/windows/win32/intl/requesting-text-recognition)

High quality offline text-to-speech:

- Google TTS: <https://stackoverflow.com/questions/63930953/google-speech-to-text-available-offline>
  - <https://cloud.google.com/speech-to-text/ondevice>
    > Private feature\
    > This product is a private feature. The documentation is publicly available but you must contact Google for full access.
    >
    > Cloud Speech-to-Text On Device enables server-quality speech technology on embedded devices. This feature allows you to run streaming speech recognition fully on-device, without any connection to a network or Google servers.
- Microsoft TTS: <https://stackoverflow.com/questions/78184469/can-i-use-the-narrator-natural-voices-added-in-windows-11-in-system-speech-synth>
  - [What is embedded speech?](https://learn.microsoft.com/en-us/azure/ai-services/speech-service/embedded-speech?tabs=windows-target%2Cjre&pivots=programming-language-csharp)
  - [Limited Access to embedded Speech](https://learn.microsoft.com/en-us/legal/cognitive-services/speech-service/embedded-speech/limited-access-embedded-speech?context=%2Fazure%2Fai-services%2Fspeech-service%2Fcontext%2Fcontext)
    > Embedded Speech is designed for on-device speech to text and text to speech scenarios where cloud connectivity is intermittent or unavailable. Microsoft's embedded Speech feature is a Limited Access feature available by registration only, and only for certain use cases.
  - [exgd0419/NaturalVoiceSAPIAdapter - Hack to make Azure natural TTS voices accessible to any SAPI 5-compatible application](https://github.com/gexgd0419/NaturalVoiceSAPIAdapter)
- <https://www.reddit.com/r/androidapps/comments/1fzu0vu/local_offline_neural_texttospeech_on_android/>
  - <https://github.com/rhasspy/piper>: A fast, local neural text to speech system
    - Related search found:
      - <https://github.com/thewh1teagle/piper-rs>
        - Simple CLI: <https://crates.io/crates/piper-rs-cli>
      - <https://users.rust-lang.org/t/text-to-speech-for-rust/110824>
  - <https://github.com/k2-fsa/sherpa-onnx>: Speech-to-text, text-to-speech, speaker diarization, and VAD using next-gen Kaldi with onnxruntime without Internet connection
    - <https://github.com/thewh1teagle/sherpa-rs>: Rust bindings to `k2-fsa/sherpa-onnx`

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
