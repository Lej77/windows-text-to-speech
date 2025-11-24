# Windows text-to-speech

This repository contains a Rust CLI program that uses Windows' text-to-speech APIs to read text passed to the program. You can find the source code in `./crates/windows_tts_cli/`.

This repository also contains a text-to-speech engine which uses Microsoft Language Detection to determine the sent language and then selects a voice in that language and plays the text using the `Windows.Media.SpeechSynthesis.SpeechSynthesizer` class. The engine can alternatively use the [`lingua`](https://crates.io/crates/lingua) Rust library for better language detection and/or the [`piper-rs`](https://crates.io/crates/piper-rs) Rust library for text-to-speech.

## Usage of `windows_tts_cli`

[Install Rust](https://www.rust-lang.org/tools/install) and then you can install this program from source:

```powershell
cargo install --git "https://github.com/Lej77/windows-text-to-speech" windows_tts_cli
# Latest "windows_tts_cli.exe" will be built and placed inside "%UserProfile%/.cargo/bin/"
windows_tts_cli.exe This text will be read

cargo uninstall windows_tts_cli
```

If you have cloned this repository, then you can run the code using:

```powershell
cargo run -- This text will be read
```

Alternatively download the `windows_tts_cli.exe` binary from the [latest release](https://github.com/Lej77/windows-text-to-speech/releases) and run that from the command line:

```powershell
./windows_tts_cli.exe This text will be read
```

If you have [Cargo B(inary)Install](https://github.com/cargo-bins/cargo-binstall) then it can download the latest release for you:

```powershell
cargo binstall --git "https://github.com/Lej77/windows-text-to-speech" windows_tts_cli
# Latest "windows_tts_cli.exe" will be downloaded to "%UserProfile%/.cargo/bin/"
windows_tts_cli.exe This text will be read

cargo uninstall windows_tts_cli
```

## Usage of `windows_tts_engine`

1. Acquire `windows_tts_engine_installer.exe` and at least one text-to-speech engine like `windows_tts_engine.dll` or `windows_tts_engine_piper.dll`.
   - You can find them in the [latest GitHub release](https://github.com/Lej77/windows-text-to-speech/releases).
     - The `windows_tts_engine_piper_lingua.dll` file is an alternative to `windows_tts_engine_piper.dll` and should be renamed to that before running the installer.
       - This alternative DLL uses the [`lingua`](https://crates.io/crates/lingua) Rust library for language detection.
       - The DLL has a larger file size and the `lingua` will also use more RAM but it should be slightly better at detecting languages.
   - Or you can build them from source:
     1. [Install Rust](https://www.rust-lang.org/tools/install)
     2. Clone this repository:\
        `git clone https://github.com/Lej77/windows-text-to-speech.git`
     3. Build everything in the repository:\
        `cargo build --release --workspace`
     4. You should find the built files inside the `./target/release` folder.
2. Place all files in the same directory and run the installer.
   - Actually you don't need the installer, just run `regsvr32 ./windows_tts_engine.dll` for each of the text-to-speech engine DLLs to install them.
     - This won't add an uninstall entry in Windows Settings app.
     - This command needs to run with admin rights, otherwise it will fail.
3. You can now find and select the voice in Windows Control Panel under the `Speech Recognition` icon then the `Text to Speech` link in the left sidebar.
   - The text-to-speech engine will **NOT** be visible in the modern Settings app under the\
      `Time & Languages` > `Speech` > `Voices` option.
   - The text-to-speech engine **WILL** be visible in the modern Settings app under\
     `Accessibility` > `Narrator` > `Choose a voice` option.
4. If you move the files you need to re-install them, otherwise Windows won't be able to find them.
5. Uninstall the program using the command `windows_tts_engine_installer.exe --uninstall` or through Windows Settings app (the `Programs and Features` panel).
   - Note that the uninstaller won't remove any files, it will only unregister the program from Windows by removing Windows Registry entries.
   - If you installed the text-to-speech engine without the install then you can uninstall it using `regsvr32 /u ./windows_tts_engine.dll`. (Use the full path if the terminal isn't in the same folder as the dll file.)
     - This command needs to run with admin rights, otherwise it will fail.

If you installed the `windows_tts_engine_piper.dll` text-to-speech engine then it will expect a folder named `piper_models` inside the same folder as the DLL file. In the `piper_models` folder you need to put `.onnx.json` model configs and `.onnx` model files for the engine to work. You can also add `.voice.txt` files next to the model files with a single integer in each to specify the voice/speaker used (for models with multiple speakers).

Example file structure:

- `C:\Program Files\Lej77TextToSpeech`
  - `piper_models`
    - `en_US-libritts_r-medium.onnx`
    - `en_US-libritts_r-medium.onnx.json`
    - `en_US-libritts_r-medium.voice.txt` (optional)
  - `windows_tts_engine.dll`
  - `windows_tts_engine_installer.exe`
  - `windows_tts_engine_piper.dll`
  - `windows_tts_engine_piper.debug.log` (only when debugging, will grow in size without any limit)

### Debugging text-to-speech engine DLL

Both text-to-speech engine DLL can write debug logs if there is a `DLL_NAME.debug.log` file present next to the engine DLL. This is useful if the text to speech engine is not working properly and you want to determine why. Make sure to delete the log file after you finish debugging since otherwise the engine will keep writing debug logs into it forever, which might eventually make it quite large.

### Prerequisites for `windows_tts_engine_piper.dll`

The `windows_tts_engine_piper.dll` DLL is not statically linked to the C runtime so to use it you need to install the [`Microsoft Visual C++ Runtime`](https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist?view=msvc-170).

The piper text-to-speech engine also requires eSpeak NG data files. You can download them from [`piper-rs`'s GitHub releases](https://github.com/thewh1teagle/piper-rs/releases/tag/espeak-ng-files) or by simply [installing eSpeak NG](https://github.com/espeak-ng/espeak-ng/releases) itself.

## References

### Text-to-speech on Windows

- There are two APIs: [text to speech - Windows 10 TTS voices not showing up? - Stack
  Overflow](https://stackoverflow.com/questions/40406719/windows-10-tts-voices-not-showing-up/40427509#40427509)
- Legacy API: [Text-to-Speech Tutorial (SAPI 5.3) | Microsoft
  Learn](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ms720163(v=vs.85))
- Modern API: [Windows.Media.SpeechSynthesis Namespace - Windows apps | Microsoft
  Learn](https://learn.microsoft.com/en-us/uwp/api/windows.media.speechsynthesis?view=winrt-26100&redirectedfrom=MSDN)
  - [SpeechSynthesizer Class (Windows.Media.SpeechSynthesis) - Windows apps | Microsoft Learn](https://learn.microsoft.com/en-us/uwp/api/windows.media.speechsynthesis.speechsynthesizer?view=winrt-26100#examples) (C++ example code)
    - Remarks mention that:
      > Only Microsoft-signed voices installed on the system can be used to generate speech.

      So it is likely not easy to develop new voices for this API.
  - Rust library that uses this API: [tts 0.26.3 - Docs.rs](https://docs.rs/crate/tts/latest/source/src/backends/winrt.rs)
- Detect language: [Microsoft Language Detection - Win32 apps | Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/intl/microsoft-language-detection)
  - [About Extended Linguistic Services - Win32 apps | Microsoft Learn](https://learn.microsoft.com/pl-pl/windows/win32/intl/about-extended-linguistic-services)
  - [Requesting Text Recognition - Win32 apps | Microsoft Learn](https://learn.microsoft.com/pl-pl/windows/win32/intl/requesting-text-recognition) (C++ example code)

### High quality offline text-to-speech

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
- <https://github.com/rhasspy/piper>: A fast, local neural text to speech system
  - Test it at: <https://piper.ttstool.com/>
  - Recommended at: <https://www.reddit.com/r/androidapps/comments/1fzu0vu/local_offline_neural_texttospeech_on_android/>
  - Related search found:
    - [`piper-rs` crate](https://crates.io/crates/piper-rs/)
      - The [`piper-rs-cli` crate](https://crates.io/crates/piper-rs-cli) offers a simple CLI
    - <https://users.rust-lang.org/t/text-to-speech-for-rust/110824>
    - [`piper-tts-rust` crate](https://crates.io/crates/piper-tts-rust)
      - Doesn't use `espeak-ng` for phonemization, instead uses [cisco-ai/mini-bart-g2p](https://huggingface.co/cisco-ai/mini-bart-g2p) and so only supports English
      - Mentions it considered using [`CMUdict`](https://github.com/cmusphinx/cmudict) for phonemization (also only supports English)
      - Perhaps it could be ported to use a model such as [lingjzhu/CharsiuG2P: Multilingual G2P in 100 languages](https://github.com/lingjzhu/CharsiuG2P)
- [`Kokoro` TTS model](https://huggingface.co/hexgrad/Kokoro-82M):
  - [`sherpa-rs` crate](https://crates.io/crates/sherpa-rs): Rust bindings to [`k2-fsa/sherpa-onnx`](https://github.com/k2-fsa/sherpa-onnx)
    - <https://github.com/k2-fsa/sherpa-onnx>: Speech-to-text, text-to-speech, speaker diarization, and VAD using next-gen Kaldi with onnxruntime without Internet connection
    - [TTS models - sherpa-onnx text-to-speech samples](https://k2-fsa.github.io/sherpa/onnx/tts/all/)
    - Kokoro model supported by sherpa: [Kokoro TTS works now in Rust : r/rust](https://www.reddit.com/r/rust/comments/1i4kqmv/kokoro_tts_works_now_in_rust/)
  - [lucasjinreal/Kokoros: 🔥🔥 Kokoro in Rust. https://huggingface.co/hexgrad/Kokoro-82M Insanely fast, realtime TTS with high quality you ever have.](https://github.com/lucasjinreal/Kokoros)
    - Forked as: [WismutHansen/kokorox: Kokoro in Rust. https://huggingface.co/hexgrad/Kokoro-82M Insanely fast, realtime TTS with high quality you ever have.](https://github.com/wismuthansen/kokorox)
      - Published as crate [`kokorox`](https://crates.io/crates/kokorox)
      - Seems to focus on supporting languages other than English.
  - [`kokoro-tts` crate](https://lib.rs/crates/kokoro-tts) (Chinese Readme) (deps: [`ort`](https://crates.io/crates/ort)) (doesn't seem to rely on `espeak`, instead seems to manually do phonemization of Chinese using [`pinyin`](https://crates.io/crates/pinyin) and [`jieba-rs`](https://crates.io/crates/jieba-rs))
  - [`kokoroxide` crate](https://crates.io/crates/kokoroxide) (WIP, mentions English only so far) (deps: [`espeak-ng`](https://github.com/espeak-ng/espeak-ng) (links directly), [`ort`](https://crates.io/crates/ort))
  - [`kokoro-tiny` crate](https://crates.io/crates/kokoro-tiny) (deps: [`espeak-rs`](https://crates.io/crates/espeak-rs), [`ort`](https://crates.io/crates/ort))
- [supertone-inc/supertonic: Lightning-fast, on-device TTS — running natively via ONNX.](https://github.com/supertone-inc/supertonic)
  - Mentioned on Reddit: [Open-source on-device TTS model : r/rust](https://www.reddit.com/r/rust/comments/1p4ohus/opensource_ondevice_tts_model/)

### Develop new text-to-speech voices/engines for **legacy** Microsoft Speech API (SAPI)

- Has some useful links: [How to make a new SAPI voice for text-to-speech? - Stack Overflow](https://stackoverflow.com/questions/22881861/how-to-make-a-new-sapi-voice-for-text-to-speech)

- Guide: [TTS Engine Vendor Porting Guide (SAPI 5.3) | Microsoft Learn](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ms717037(v=vs.85)?redirectedfrom=MSDN)

- Interface reference: [Text-to-speech recognition engine manager (DDI-level) (SAPI 5.3) | Microsoft Learn](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ms717235(v=vs.85))

- Example: [eSpeak: speech synthesis - Browse /espeak/espeak-1.48 at SourceForge.net](https://sourceforge.net/projects/espeak/files/espeak/espeak-1.48/)

  - Note: `eSpeak` voices don't seem to work on 64bit systems. Their registry keys are added to `HKLM\SOFTWARE\WOW6432Node\Microsoft\SPEECH\Voices\Tokens\eSpeak` and even if the `WOW6432Node` path segment is removed Windows won't find the COM service anyway (but the entries will show up in the Control Panel's selector of default SAPI voice).

  - Note: the modern `eSpeak-ng` doesn't support SAPI yet, but the issue has some useful info:\
    [Reimplement the SAPI bindings. · Issue #7 · espeak-ng/espeak-ng](https://github.com/espeak-ng/espeak-ng/issues/7#issuecomment-2527109323)

    - Links to this project which installs SAPI voices: [gexgd0419/NaturalVoiceSAPIAdapter](https://github.com/gexgd0419/NaturalVoiceSAPIAdapter)

- [Add new TTS technology/project (Coqui / Piper TTS) to SAPI - Microsoft Q&A](https://learn.microsoft.com/en-us/answers/questions/1444682/add-new-tts-technology-project-(coqui-piper-tts)-t)

  - [Sample Engines (SAPI 5.3) | Microsoft Learn](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ms720179(v=vs.85))

- Somewhat related: [How to add custom SR (Speech Recognition) to Microsoft SAPI - Stack Overflow](https://stackoverflow.com/questions/16851914/how-to-add-custom-sr-speech-recognition-to-microsoft-sapi)

- [System.Speech.Synthesis.TtsEngine Namespace | Microsoft Learn](https://learn.microsoft.com/en-us/dotnet/api/system.speech.synthesis.ttsengine?view=net-9.0-pp) (see Remarks for "guide")

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
