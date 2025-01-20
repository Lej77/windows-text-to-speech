//! Safer bindings for building a Text-to-speech engine for Windows using the
//! [Speech Application Programming Interface (SAPI)](https://en.wikipedia.org/wiki/Microsoft_Speech_API).
//!
//! # References
//!
//! - Guide: [TTS Engine Vendor Porting Guide (SAPI 5.3) | Microsoft Learn](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ms717037(v=vs.85)?redirectedfrom=MSDN)
//! - Interfaces docs: [Text-to-speech recognition engine manager (DDI-level) (SAPI 5.3) | Microsoft Learn](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ms717235(v=vs.85))
//! - Installing an engine: [Sample Engines (SAPI 5.3) | Microsoft Learn](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ms720179(v=vs.85))
//! - [System.Speech.Synthesis.TtsEngine Namespace | Microsoft Learn](https://learn.microsoft.com/en-us/dotnet/api/system.speech.synthesis.ttsengine?view=net-9.0-pp)

use std::sync::Arc;

use windows::Win32::Media::{
    Audio::WAVEFORMATEX,
    Speech::{ISpObjectToken, ISpTTSEngineSite, SPVSTATE, SPVTEXTFRAG},
};
use windows_core::GUID;

pub mod com_server;
pub mod detect_languages;
pub mod logging;
pub mod utils;
pub mod voices;

// Re-export of `windows` crate.
pub use windows;

/// Linked list of text fragments to synthesize.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct TextFrag<'a>(&'a SPVTEXTFRAG);
impl<'a> TextFrag<'a> {
    /// Wrap a fragment list.
    ///
    /// # Safety
    ///
    /// - The pointer must be null or point to a valid element where all
    ///   pointers also point to other valid structures and the length field is
    ///   correct.
    /// - The lifetime of the returned type must be constrained so that the
    ///   pointed to value isn't accessed after it is freed.
    pub unsafe fn new(ptextfraglist: *const SPVTEXTFRAG) -> Option<Self> {
        if ptextfraglist.is_null() {
            None
        } else {
            Some(TextFrag(unsafe { &*ptextfraglist }))
        }
    }
    /// Next text fragment.
    pub fn next(self) -> Option<TextFrag<'a>> {
        unsafe { Self::new(self.0.pNext) }
    }
    /// The text string associated with this fragment.
    pub fn utf16_text(self) -> &'a [u16] {
        assert!(
            !self.0.pTextStart.is_null(),
            "Text fragment should not be null"
        );
        unsafe {
            core::slice::from_raw_parts(self.0.pTextStart.as_ptr(), self.0.ulTextLen as usize)
        }
    }
    /// Original character offset of [`Self::utf16_text`] within the text string
    /// passed to `ISpVoice::Speak`.
    pub fn offset_in_original_text(self) -> u32 {
        self.0.ulTextSrcOffset
    }
    /// The current XML attribute state.
    pub fn state(self) -> &'a SPVSTATE {
        &self.0.State
    }

    /// Iterator over this fragment and all following fragments.
    pub fn iter(self) -> TextFragIter<'a> {
        TextFragIter(Some(self))
    }

    /// Debug formatting that includes information about all fragments with this
    /// fragment as the first in the list.
    pub fn debug_list(self) -> impl std::fmt::Debug + 'a {
        struct ListDebug<'a>(TextFrag<'a>);
        impl std::fmt::Debug for ListDebug<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_list().entries(self.0.iter()).finish()
            }
        }
        ListDebug(self)
    }
}
/// Writes information about this fragment and nothing about any of the other in
/// the linked list, for debug info about the whole list use
/// [`TextFrag::debug_list`].
impl std::fmt::Debug for TextFrag<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextFrag")
            .field("text", &String::from_utf16_lossy(self.utf16_text()))
            .field("offset_in_original_text", &self.offset_in_original_text())
            .field("state", self.state())
            .finish()
    }
}
impl<'a> IntoIterator for TextFrag<'a> {
    type IntoIter = TextFragIter<'a>;
    type Item = TextFrag<'a>;

    fn into_iter(self) -> Self::IntoIter {
        TextFragIter(Some(self))
    }
}

#[derive(Clone, Debug)]
pub struct TextFragIter<'a>(Option<TextFrag<'a>>);
impl<'a> TextFragIter<'a> {
    pub const fn new(frag: Option<TextFrag<'a>>) -> Self {
        Self(frag)
    }
}
impl<'a> Iterator for TextFragIter<'a> {
    type Item = TextFrag<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.0?;
        self.0 = current.next();
        Some(current)
    }
}

#[derive(Clone, Copy)]
pub enum SpeechFormat {
    /// Engines are not required to support this format, nor are they required
    /// to do anything specific with this format if they do support it. It is
    /// provided merely for debugging purposes.
    DebugText,
    Wave(WAVEFORMATEX),
}

impl std::fmt::Debug for SpeechFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DebugText => write!(f, "DebugText"),
            Self::Wave(info) => f
                .debug_struct("Wave")
                .field("wFormatTag", &{ info.wFormatTag })
                .field("nChannels", &{ info.nChannels })
                .field("nSamplesPerSec", &{ info.nSamplesPerSec })
                .field("nAvgBytesPerSec", &{ info.nAvgBytesPerSec })
                .field("nBlockAlign", &{ info.nBlockAlign })
                .field("wBitsPerSample", &{ info.wBitsPerSample })
                .field("cbSize", &{ info.cbSize })
                .finish(),
        }
    }
}

/// Used by [`WindowsTtsEngine`] to implement COM interfaces such as
/// [`ISpTTSEngine`](windows::Win32::Media::Speech::ISpTTSEngine).
///
/// # Thread safety
///
/// TTS engine instances will always be called by SAPI on a single thread, so
/// [`Sync`] isn't required. More info at: [ISpTTSEngine (SAPI 5.4) | Microsoft
/// Learn](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ee413466(v=vs.85))
pub trait SafeTtsEngine: Send + 'static {
    /// Engines may implement this in order to access their object token data.
    ///
    /// For more info, see:
    /// [ISpObjectWithToken (SAPI 5.4) | Microsoft Learn](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ee450882(v=vs.85))
    fn set_object_token(&self, _token: &ISpObjectToken) -> windows_core::Result<()> {
        Ok(())
    }

    /// Renders the specified text fragment list in the specified output format.
    ///
    /// If `speak_punctuation` is `true` then the engine should speak all
    /// punctuation (e.g., "This is a sentence." should be expanded to "This is
    /// a sentence period").
    ///
    /// `wave_format` is guaranteed to be one that the engine specified as
    /// supported in a previous [`SafeTtsEngine::get_output_format`] call.
    ///
    /// Audio data and events should be written to `output_site`.
    fn speak(
        &self,
        _token: &ISpObjectToken,
        speak_punctuation: bool,
        wave_format: SpeechFormat,
        text_fragments: Option<TextFrag<'_>>,
        output_site: &ISpTTSEngineSite,
    ) -> windows_core::Result<()>;

    /// The engine should examine the requested output format, and return the
    /// closest format that it supports.
    ///
    /// If `target_format` is `None` then the caller does not care about the
    /// target format and the engine can return any format that it supports.
    fn get_output_format(
        &self,
        _token: &ISpObjectToken,
        target_format: Option<SpeechFormat>,
    ) -> windows_core::Result<SpeechFormat>;
}

mod private_impls {
    //! Inner module to make the generated [`WindowsTtsEngine_Impl`] type
    //! private since its trait implementation has methods that should be unsafe
    //! to call.

    use crate::{
        utils::{catch_unwind_and_fail, safe_catch_unwind},
        SafeTtsEngine, SpeechFormat, TextFrag,
    };
    use core::ffi::c_void;
    use std::{
        ptr::{self, null_mut},
        sync::{Arc, OnceLock},
    };

    use windows::Win32::{
        Foundation::{
            BOOL, CLASS_E_NOAGGREGATION, E_FAIL, E_INVALIDARG, E_NOINTERFACE, E_NOTIMPL,
            E_OUTOFMEMORY, E_POINTER,
        },
        Media::{
            Audio::WAVEFORMATEX,
            Speech::{
                ISpObjectToken, ISpObjectWithToken, ISpObjectWithToken_Impl, ISpTTSEngine,
                ISpTTSEngineSite, ISpTTSEngine_Impl, SPF_NLP_SPEAK_PUNC, SPVTEXTFRAG,
            },
        },
        System::Com::{CoTaskMemAlloc, IClassFactory, IClassFactory_Impl},
    };
    use windows_core::{implement, IUnknown, Interface, Ref, GUID};

    // https://docs.rs/winapi/latest/src/winapi/um/sapi51.rs.html#115
    unsafe extern "C" {
        /// `7CEEF9F9-3D13-11D2-9EE7-00C04F797396`
        pub safe static SPDFID_Text: GUID;
        /// `C31ADBAE-527F-4FF5-A230-F62BB61FF70C`
        pub safe static SPDFID_WaveFormatEx: GUID;
    }

    #[implement(IClassFactory)]
    pub struct WindowsTtsEngineFactory {
        pub(super) tts_engine_class_id: GUID,
        pub(super) module_ref: Option<Arc<()>>,
        pub(super) create_tts_engine: Box<dyn Fn() -> Box<dyn SafeTtsEngine> + Send + Sync>,
    }

    /// Required for Windows to create and start our service when a client requests it.
    ///
    /// - [COM Class Objects and CLSIDs - Win32 apps | Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/com/com-class-objects-and-clsids)
    /// - [Self-Registration - Win32 apps | Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/com/self-registration)
    /// - [rust - Implementing a Windows Credential Provider - Stack Overflow](https://stackoverflow.com/questions/75279682/implementing-a-windows-credential-provider)
    impl IClassFactory_Impl for WindowsTtsEngineFactory_Impl {
        fn CreateInstance(
            &self,
            punkouter: Ref<'_, IUnknown>,
            riid: *const GUID,
            ppvobject: *mut *mut c_void,
        ) -> windows_core::Result<()> {
            // Validate arguments
            if ppvobject.is_null() {
                return Err(E_POINTER.into());
            }
            unsafe { ppvobject.write(ptr::null_mut()) };
            if riid.is_null() {
                return Err(E_INVALIDARG.into());
            }
            let riid = unsafe { riid.read() };
            if punkouter.is_some() {
                return Err(CLASS_E_NOAGGREGATION.into());
            }

            // We're only handling requests for a specific class id or any interface it implements
            if ![
                self.tts_engine_class_id,
                IUnknown::IID,
                ISpTTSEngine::IID,
                ISpObjectWithToken::IID,
            ]
            .contains(&riid)
            {
                return Err(E_NOINTERFACE.into());
            }

            let engine: *mut c_void = catch_unwind_and_fail(|| {
                // Construct the engine:
                let safe_engine = (self.create_tts_engine)();
                let engine = WindowsTtsEngine::new_boxed(safe_engine, self.module_ref.clone());
                // Cast it into the COM interface it implements:
                Ok(
                    if riid == self.tts_engine_class_id || riid == ISpTTSEngine::IID {
                        ISpTTSEngine::from(engine).into_raw()
                    } else if IUnknown::IID == riid {
                        IUnknown::from(engine).into_raw()
                    } else if ISpObjectWithToken::IID == riid {
                        ISpObjectWithToken::from(engine).into_raw()
                    } else {
                        unreachable!(
                            "we already guarded against unknown ids and returned E_NOINTERFACE"
                        )
                    },
                )
            })?;
            // Return the COM interface in the out pointer:
            unsafe { ppvobject.write(engine) };
            Ok(())
        }

        fn LockServer(&self, _flock: BOOL) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
    }

    #[implement(ISpTTSEngine, ISpObjectWithToken)]
    pub struct WindowsTtsEngine {
        pub(super) engine: Box<dyn SafeTtsEngine>,
        pub(super) module_ref: Option<Arc<()>>,
        pub(super) token: OnceLock<ISpObjectToken>,
    }

    /// We need this interface according to
    /// [TTS Engine Vendor Porting Guide SAPI 5.4 | Microsoft Learn](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ee431802(v=vs.85)).
    impl ISpObjectWithToken_Impl for WindowsTtsEngine_Impl {
        fn SetObjectToken(&self, ptoken: Ref<'_, ISpObjectToken>) -> windows_core::Result<()> {
            if self.token.set(ptoken.unwrap().clone()).is_err() {
                safe_catch_unwind(|| {
                    log::error!("ISpObjectWithToken::SetObjectToken was called twice")
                });
                return Err(E_FAIL.into());
            }
            catch_unwind_and_fail(move || self.engine.set_object_token(ptoken.unwrap()))
        }

        fn GetObjectToken(&self) -> windows_core::Result<ISpObjectToken> {
            self.token.get().cloned().ok_or_else(|| {
                // not set any token
                safe_catch_unwind(|| {
                    log::error!("ISpObjectWithToken::GetObjectToken called before SetObjectToken")
                });
                E_FAIL.into()
            })
        }
    }

    impl ISpTTSEngine_Impl for WindowsTtsEngine_Impl {
        fn Speak(
            &self,
            dwspeakflags: u32,
            rguidformatid: *const GUID,
            pwaveformatex: *const WAVEFORMATEX,
            ptextfraglist: *const SPVTEXTFRAG,
            poutputsite: Ref<'_, ISpTTSEngineSite>,
        ) -> windows_core::Result<()> {
            catch_unwind_and_fail(move || {
                // Replace "." with " period "
                let speak_punctuation = (dwspeakflags as i32) & SPF_NLP_SPEAK_PUNC.0 != 0;

                let format_id = unsafe { *rguidformatid };

                let wave_format_ex = if pwaveformatex.is_null() {
                    None
                } else {
                    Some(unsafe { &*pwaveformatex })
                };

                let frag_list = unsafe { TextFrag::new(ptextfraglist) };

                let wave_format = if let Some(format) = wave_format_ex {
                    debug_assert_eq!(format_id, SPDFID_WaveFormatEx);
                    SpeechFormat::Wave(*format)
                } else {
                    debug_assert_eq!(
                        format_id, SPDFID_Text,
                        "if no wave format then it is because the output is debug text"
                    );
                    SpeechFormat::DebugText
                };

                self.engine.speak(
                    self.token.get().ok_or_else(|| {
                        log::error!(
                            "ISpTTSEngine::Speak called before ISpObjectWithToken::SetObjectToken"
                        );
                        E_FAIL
                    })?,
                    speak_punctuation,
                    wave_format,
                    frag_list,
                    poutputsite.unwrap(),
                )?;

                Ok(())
            })
        }

        fn GetOutputFormat(
            &self,
            ptargetfmtid: *const GUID,
            ptargetwaveformatex: *const WAVEFORMATEX,
            poutputformatid: *mut GUID,
            ppcomemoutputwaveformatex: *mut *mut WAVEFORMATEX,
        ) -> windows_core::Result<()> {
            catch_unwind_and_fail(move || {
                let target_format_id = if ptargetfmtid.is_null() {
                    None
                } else {
                    Some(unsafe { &*ptargetfmtid })
                };

                let target_wave_format_ex = if ptargetwaveformatex.is_null() {
                    None
                } else {
                    Some(unsafe { &*ptargetwaveformatex })
                };
                debug_assert!(
                    target_wave_format_ex.is_some()
                        || target_format_id.map_or(true, |format| *format == SPDFID_Text),
                );

                let target_format = match target_wave_format_ex {
                    Some(format) => {
                        debug_assert_eq!(target_format_id, Some(&SPDFID_WaveFormatEx));
                        Some(SpeechFormat::Wave(*format))
                    }
                    None if target_format_id.is_some() => {
                        debug_assert_eq!(
                            target_format_id,
                            Some(&SPDFID_Text),
                            "if no wave format was requested then it is because the output \
                            is debug text or because no specific format was requested"
                        );
                        Some(SpeechFormat::DebugText)
                    }
                    None => None,
                };

                match self.engine.get_output_format(
                    self.token.get().ok_or_else(|| {
                        log::error!(
                            "ISpTTSEngine::GetOutputFormat called before \
                            ISpObjectWithToken::SetObjectToken"
                        );
                        E_FAIL
                    })?,
                    target_format,
                ) {
                    Err(e) => {
                        // Write to out arguments to be as safe as possible:
                        unsafe {
                            poutputformatid.write(GUID::zeroed());
                            ppcomemoutputwaveformatex.write(null_mut());
                        }
                        return Err(e);
                    }
                    Ok(SpeechFormat::DebugText) => unsafe {
                        poutputformatid.write(SPDFID_Text);
                        ppcomemoutputwaveformatex.write(null_mut());
                    },
                    Ok(SpeechFormat::Wave(mut wanted_format)) => unsafe {
                        wanted_format.cbSize = 0; // Extra information after structure (we haven't allocated any extra space)

                        let allocated =
                            CoTaskMemAlloc(size_of::<WAVEFORMATEX>()).cast::<WAVEFORMATEX>();

                        if allocated.is_null() {
                            poutputformatid.write(GUID::zeroed());
                            ppcomemoutputwaveformatex.write(null_mut());

                            return Err(E_OUTOFMEMORY.into());
                        }
                        allocated.write(wanted_format);

                        poutputformatid.write(SPDFID_WaveFormatEx);
                        ppcomemoutputwaveformatex.write(allocated);
                    },
                }

                Ok(())
            })
        }
    }
}
pub use private_impls::{WindowsTtsEngine, WindowsTtsEngineFactory};

impl WindowsTtsEngine {
    pub fn new<T: SafeTtsEngine>(engine: T, module_ref: Option<Arc<()>>) -> Self {
        Self::new_boxed(Box::new(engine), module_ref)
    }
    pub fn new_boxed(engine: Box<dyn SafeTtsEngine>, module_ref: Option<Arc<()>>) -> Self {
        Self {
            engine,
            module_ref,
            token: std::sync::OnceLock::new(),
        }
    }
}
impl Drop for WindowsTtsEngine {
    fn drop(&mut self) {
        // TODO: drop user type so that it doesn't panic out of the COM
        // wrapper's free function.
        log::debug!(
            "WindowsTtsEngine was dropped, module_refs: {}",
            if let Some(count) = self.module_ref.as_ref().map(Arc::strong_count) {
                count.to_string()
            } else {
                "untracked".to_string()
            }
        );
    }
}

impl WindowsTtsEngineFactory {
    pub fn new<T: SafeTtsEngine>(
        engine_class_id: GUID,
        module_ref: Option<Arc<()>>,
        create_engine: impl Fn() -> T + Send + Sync + 'static,
    ) -> Self {
        Self {
            tts_engine_class_id: engine_class_id,
            module_ref,
            create_tts_engine: Box::new(move || Box::new(create_engine())),
        }
    }
}
impl Drop for WindowsTtsEngineFactory {
    fn drop(&mut self) {
        // TODO: drop user type so that it doesn't panic out of the COM
        // wrapper's free function.
        log::debug!(
            "WindowsTtsEngineFactory was dropped, module_refs: {}",
            if let Some(count) = self.module_ref.as_ref().map(Arc::strong_count) {
                count.to_string()
            } else {
                "untracked".to_string()
            }
        );
    }
}
