use crate::Adapter;
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::jstring,
};
use std::sync::{Mutex, OnceLock};

static ADAPTER: OnceLock<Mutex<Adapter>> = OnceLock::new();

/// Called by the Android service. No pointer is retained after the call.
// SAFETY: the JVM invokes this exported symbol with the matching static Java
// signature (String -> String). JNI wrappers validate local object conversions;
// all references stay on this calling thread and within this invocation.
#[allow(unsafe_code)] // Sole JNI export; ABI safety is documented above.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_freetak_atn_NativeCore_dispatch(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    request: JString<'_>,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let text: String = env
            .get_string(&request)
            .map_err(|_| "invalid Java string")?
            .into();
        let mut adapter = ADAPTER
            .get_or_init(|| Mutex::new(Adapter::new()))
            .lock()
            .map_err(|_| "adapter lock poisoned; restart plugin")?;
        Ok::<_, &str>(adapter.dispatch(&text))
    }));
    let response = match result {
        Ok(Ok(response)) => response,
        Ok(Err(message)) => serde_json::json!({"ok":false,"error":{"code":"jni","message":message},"actions":[{"kind":"cancelWrites"},{"kind":"cancelHostRequests","keepRequestIds":[]},{"kind":"disconnect"}]}).to_string(),
        Err(_) => {
            // A panic cannot authorize queued writes after a partial state mutation.
            if let Ok(mut adapter) = ADAPTER.get_or_init(|| Mutex::new(Adapter::new())).lock() {
                *adapter = Adapter::new();
            }
            serde_json::json!({"ok":false,"error":{"code":"panic","message":"native failure; reconnect explicitly"},"actions":[{"kind":"cancelWrites"},{"kind":"cancelHostRequests","keepRequestIds":[]},{"kind":"disconnect"}]}).to_string()
        }
    };
    match env.new_string(response) {
        Ok(value) => value.into_raw(),
        Err(_) => std::ptr::null_mut(), // JVM allocation failure retains its exception.
    }
}
