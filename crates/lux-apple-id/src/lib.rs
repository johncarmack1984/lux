//! The native Sign in with Apple sheet, as a callback.
//!
//! [`authorize`] must be called on the main thread (Tauri:
//! `app.run_on_main_thread`); the completion fires later on the main run loop
//! with the sheet's outcome. The caller supplies the SHA-256 (hex) of a raw
//! nonce it keeps: Apple embeds that digest in the identity token's `nonce`
//! claim, and the backend re-hashes the raw value to bind token and sheet run.
//!
//! Platform reality: real on macOS and iOS (the entitlement decides whether
//! the sheet actually works at runtime); a compile-clean stub that errors
//! immediately everywhere else, so callers need no cfg of their own.

/// What a completed sheet hands back. `email`/`full_name` are only present on
/// the FIRST authorization for this Apple ID — forward them to the backend,
/// which persists them then or never.
#[derive(Debug)]
pub struct Authorization {
    /// Apple's identity token (a JWT), UTF-8.
    pub identity_token: String,
    /// The single-use, ~5-minute authorization code, UTF-8.
    pub authorization_code: String,
    pub email: Option<String>,
    pub full_name: Option<String>,
}

/// The user dismissed the sheet. Matched by callers to stay quiet about it.
pub const CANCELED: &str = "canceled";

pub type OnDone = Box<dyn FnOnce(Result<Authorization, String>) + Send>;

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use platform::authorize;

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub fn authorize(_hashed_nonce: &str, on_done: OnDone) {
    on_done(Err(
        "sign in with apple is not available on this platform".into()
    ));
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod platform {
    use std::cell::RefCell;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, ProtocolObject};
    use objc2::{
        define_class, msg_send, AnyThread, ClassType, DefinedClass, MainThreadMarker,
        MainThreadOnly,
    };
    #[cfg(target_os = "macos")]
    use objc2_authentication_services::ASAuthorizationControllerPresentationContextProviding;
    use objc2_authentication_services::{
        ASAuthorization, ASAuthorizationAppleIDCredential, ASAuthorizationAppleIDProvider,
        ASAuthorizationController, ASAuthorizationControllerDelegate, ASAuthorizationScopeEmail,
        ASAuthorizationScopeFullName,
    };
    use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol, NSString};

    use super::{Authorization, OnDone, CANCELED};

    // The one in-flight (delegate, controller) pair. The controller keeps
    // itself alive during the flow, but its `delegate` property is WEAK — this
    // slot is what keeps the delegate reachable until its callback runs. A new
    // authorization replaces (and thereby releases) the previous pair; main
    // thread only, hence `thread_local`.
    thread_local! {
        static IN_FLIGHT: RefCell<Option<(Retained<Delegate>, Retained<ASAuthorizationController>)>> =
            const { RefCell::new(None) };
    }

    struct Ivars {
        /// Taken by whichever completion callback fires first.
        on_done: RefCell<Option<OnDone>>,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "LuxAppleIDDelegate"]
        #[ivars = Ivars]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl ASAuthorizationControllerDelegate for Delegate {
            #[unsafe(method(authorizationController:didCompleteWithAuthorization:))]
            fn did_complete_with_authorization(
                &self,
                _controller: &ASAuthorizationController,
                authorization: &ASAuthorization,
            ) {
                self.finish(extract(authorization));
            }

            #[unsafe(method(authorizationController:didCompleteWithError:))]
            fn did_complete_with_error(
                &self,
                _controller: &ASAuthorizationController,
                error: &NSError,
            ) {
                // ASAuthorizationError code 1001 is the user closing the sheet.
                let message = if error.code() == 1001 {
                    CANCELED.to_owned()
                } else {
                    format!(
                        "apple authorization failed ({}): {}",
                        error.code(),
                        error.localizedDescription()
                    )
                };
                self.finish(Err(message));
            }
        }

        // macOS wants told where to hang the sheet; on iOS the system uses the
        // key window when no provider is set (the generated binding only
        // carries this method on macOS).
        #[cfg(target_os = "macos")]
        unsafe impl ASAuthorizationControllerPresentationContextProviding for Delegate {
            #[unsafe(method_id(presentationAnchorForAuthorizationController:))]
            fn presentation_anchor(
                &self,
                _controller: &ASAuthorizationController,
            ) -> Retained<NSObject> {
                anchor_window()
            }
        }
    );

    impl Delegate {
        fn new(mtm: MainThreadMarker, on_done: OnDone) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(Ivars {
                on_done: RefCell::new(Some(on_done)),
            });
            // SAFETY: plain NSObject init on a freshly allocated instance.
            unsafe { msg_send![super(this), init] }
        }

        fn finish(&self, result: Result<Authorization, String>) {
            if let Some(on_done) = self.ivars().on_done.borrow_mut().take() {
                on_done(result);
            }
        }
    }

    /// Present the sheet. Main thread only (checked; errors rather than UB).
    pub fn authorize(hashed_nonce: &str, on_done: OnDone) {
        let Some(mtm) = MainThreadMarker::new() else {
            on_done(Err("authorize must be called on the main thread".into()));
            return;
        };

        let delegate = Delegate::new(mtm, on_done);

        // SAFETY: all main-thread (mtm in scope); the request is configured
        // before the controller starts, and both objects live in IN_FLIGHT
        // until the delegate callback has fired.
        unsafe {
            let provider = ASAuthorizationAppleIDProvider::new();
            let request = provider.createRequest();
            request.setRequestedScopes(Some(&NSArray::from_slice(&[
                ASAuthorizationScopeFullName,
                ASAuthorizationScopeEmail,
            ])));
            request.setNonce(Some(&NSString::from_str(hashed_nonce)));

            let controller = ASAuthorizationController::initWithAuthorizationRequests(
                ASAuthorizationController::alloc(),
                &NSArray::from_slice(&[request.as_super().as_super()]),
            );
            controller.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            #[cfg(target_os = "macos")]
            controller.setPresentationContextProvider(Some(ProtocolObject::from_ref(&*delegate)));

            controller.performRequests();
            IN_FLIGHT.with(|slot| *slot.borrow_mut() = Some((delegate, controller)));
        }
    }

    /// Copy everything out of the credential immediately — nothing
    /// Objective-C-flavored escapes the callback.
    fn extract(authorization: &ASAuthorization) -> Result<Authorization, String> {
        // SAFETY: read-only accessors on the delivered credential, still on
        // the main thread inside the delegate callback.
        unsafe {
            let credential = authorization.credential();
            let credential: &AnyObject = (*credential).as_ref();
            let Some(credential) = credential.downcast_ref::<ASAuthorizationAppleIDCredential>()
            else {
                return Err("authorization returned a non-Apple-ID credential".into());
            };
            let token = credential
                .identityToken()
                .ok_or("authorization carried no identity token")?;
            let code = credential
                .authorizationCode()
                .ok_or("authorization carried no authorization code")?;
            let full_name = credential.fullName().and_then(|components| {
                let mut parts: Vec<String> = Vec::new();
                if let Some(given) = components.givenName() {
                    parts.push(given.to_string());
                }
                if let Some(family) = components.familyName() {
                    parts.push(family.to_string());
                }
                (!parts.is_empty()).then(|| parts.join(" "))
            });
            Ok(Authorization {
                identity_token: utf8(&token.to_vec(), "identity token")?,
                authorization_code: utf8(&code.to_vec(), "authorization code")?,
                email: credential.email().map(|e| e.to_string()),
                full_name,
            })
        }
    }

    fn utf8(bytes: &[u8], what: &str) -> Result<String, String> {
        String::from_utf8(bytes.to_vec()).map_err(|_| format!("{what} was not UTF-8"))
    }

    /// The window the macOS sheet attaches to: the key window, else the main
    /// window, else a bare NSObject (the sheet then fails visibly — better
    /// than never calling back).
    #[cfg(target_os = "macos")]
    fn anchor_window() -> Retained<NSObject> {
        let Some(mtm) = MainThreadMarker::new() else {
            return NSObject::new();
        };
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        let window = app.keyWindow().or_else(|| app.mainWindow());
        match window {
            Some(window) => {
                let responder = Retained::into_super(window);
                Retained::into_super(responder)
            }
            None => NSObject::new(),
        }
    }
}
