use leptos::*;
use wasm_bindgen::JsCast;

/// Small clipboard-copy button. Swaps to a checkmark icon for 1.5 s after a successful copy.
#[component]
pub fn CopyButton(text: String) -> impl IntoView {
    let (copied, set_copied) = create_signal(false);

    let on_click = move |ev: web_sys::MouseEvent| {
        ev.stop_propagation(); // don't bubble into row click-to-navigate
        let value = text.clone();
        let set = set_copied;

        // Call clipboard API synchronously within the event handler to preserve
        // the browser's transient user activation. spawn_local defers to a
        // microtask where some browsers reject the clipboard write.
        if let Some(promise) = clipboard_write_text(&value) {
            spawn_local(async move {
                let ok = wasm_bindgen_futures::JsFuture::from(promise).await.is_ok();
                if ok {
                    set.set(true);
                    gloo_timers::future::sleep(std::time::Duration::from_millis(1500)).await;
                    set.set(false);
                }
            });
        } else if exec_command_copy(&value) {
            // Fallback for non-HTTPS contexts where Clipboard API is unavailable.
            set.set(true);
            spawn_local(async move {
                gloo_timers::future::sleep(std::time::Duration::from_millis(1500)).await;
                set.set(false);
            });
        }
    };

    view! {
        <button
            on:click=on_click
            title="Copy"
            class="ml-1 p-0.5 rounded text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 \
                   hover:bg-gray-100 dark:hover:bg-gray-800 transition-colors cursor-pointer shrink-0"
        >
            {move || if copied.get() {
                view! {
                    <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24"
                         fill="none" stroke="currentColor" stroke-width="2.5"
                         stroke-linecap="round" stroke-linejoin="round"
                         class="text-emerald-500">
                        <polyline points="20 6 9 17 4 12"/>
                    </svg>
                }.into_view()
            } else {
                view! {
                    <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24"
                         fill="none" stroke="currentColor" stroke-width="2"
                         stroke-linecap="round" stroke-linejoin="round">
                        <rect x="9" y="9" width="13" height="13" rx="2" ry="2"/>
                        <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
                    </svg>
                }.into_view()
            }}
        </button>
    }
}

/// Calls `navigator.clipboard.writeText()` synchronously, returning the JS
/// Promise. Must be called during a user-gesture event handler so the browser
/// grants clipboard access. Returns `None` when the Clipboard API is
/// unavailable (non-HTTPS, older browsers).
fn clipboard_write_text(text: &str) -> Option<js_sys::Promise> {
    let window = web_sys::window()?;
    let navigator = window.navigator();
    let clipboard_val = js_sys::Reflect::get(
        navigator.as_ref(),
        &wasm_bindgen::JsValue::from_str("clipboard"),
    )
    .ok()?;
    if clipboard_val.is_undefined() || clipboard_val.is_null() {
        return None;
    }
    let clipboard: web_sys::Clipboard = clipboard_val.unchecked_into();
    Some(clipboard.write_text(text))
}

/// Fallback copy using a temporary textarea + `document.execCommand('copy')`.
/// Works on HTTP and in older browsers where the async Clipboard API is absent.
fn exec_command_copy(text: &str) -> bool {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return false,
    };
    // Access document and body via Reflect to avoid extra web-sys feature flags.
    let document = match js_sys::Reflect::get(window.as_ref(), &"document".into()) {
        Ok(d) if !d.is_undefined() => d,
        _ => return false,
    };
    let body = match js_sys::Reflect::get(&document, &"body".into()) {
        Ok(b) if !b.is_undefined() => b,
        _ => return false,
    };

    let create_element: js_sys::Function =
        match js_sys::Reflect::get(&document, &"createElement".into()) {
            Ok(f) => f.unchecked_into(),
            _ => return false,
        };
    let textarea = match create_element.call1(&document, &"textarea".into()) {
        Ok(el) => el,
        _ => return false,
    };

    // Position off-screen and set value.
    let style = js_sys::Reflect::get(&textarea, &"style".into()).unwrap_or_default();
    let _ = js_sys::Reflect::set(&style, &"position".into(), &"fixed".into());
    let _ = js_sys::Reflect::set(&style, &"left".into(), &"-9999px".into());
    let _ = js_sys::Reflect::set(
        &textarea,
        &"value".into(),
        &wasm_bindgen::JsValue::from_str(text),
    );

    // Append, select, copy, remove.
    let append_child: js_sys::Function = match js_sys::Reflect::get(&body, &"appendChild".into()) {
        Ok(f) => f.unchecked_into(),
        _ => return false,
    };
    let _ = append_child.call1(&body, &textarea);

    let select: js_sys::Function = match js_sys::Reflect::get(&textarea, &"select".into()) {
        Ok(f) => f.unchecked_into(),
        _ => return false,
    };
    let _ = select.call0(&textarea);

    let exec_command: js_sys::Function =
        match js_sys::Reflect::get(&document, &"execCommand".into()) {
            Ok(f) => f.unchecked_into(),
            _ => return false,
        };
    let ok = exec_command
        .call1(&document, &"copy".into())
        .map(|v| v.as_bool().unwrap_or(false))
        .unwrap_or(false);

    let remove_child: js_sys::Function = match js_sys::Reflect::get(&body, &"removeChild".into()) {
        Ok(f) => f.unchecked_into(),
        _ => return false,
    };
    let _ = remove_child.call1(&body, &textarea);

    ok
}
