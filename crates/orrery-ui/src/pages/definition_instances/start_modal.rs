use std::collections::HashMap;

use leptos::*;
use orrery_types::{ProcessDefinitionVersionsResponse, StartInstanceRequest};

use crate::api;

fn parse_value(raw: &str) -> serde_json::Value {
    // Try JSON objects/arrays first so users can enter {"a":1} or [1,2,3].
    let trimmed = raw.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(v) = serde_json::from_str(raw) {
            return v;
        }
    }
    if raw == "true" {
        serde_json::Value::Bool(true)
    } else if raw == "false" {
        serde_json::Value::Bool(false)
    } else if let Ok(n) = raw.parse::<i64>() {
        serde_json::Value::Number(n.into())
    } else if let Ok(f) = raw.parse::<f64>() {
        serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .unwrap_or_else(|| serde_json::Value::String(raw.to_string()))
    } else {
        serde_json::Value::String(raw.to_string())
    }
}

/// Modal for starting a process instance with optional initial variables.
#[component]
pub fn StartModal(
    def_id: Signal<String>,
    versions: Signal<Option<ProcessDefinitionVersionsResponse>>,
    selected_version: Signal<Option<i32>>,
    on_close: Callback<()>,
    on_started: Callback<()>,
) -> impl IntoView {
    // Local version signal — initialised from the page's current version selection
    let (modal_version, set_modal_version) = create_signal(selected_version.get_untracked());

    // Each entry: (key, value) as strings
    let (business_key, set_business_key) = create_signal(String::new());
    let (rows, set_rows) = create_signal(Vec::<(String, String)>::new());
    let (new_key, set_new_key) = create_signal(String::new());
    let (new_val, set_new_val) = create_signal(String::new());
    let (submitting, set_submitting) = create_signal(false);
    let (error, set_error) = create_signal(Option::<String>::None);

    let add_row = move || {
        let k = new_key.get_untracked();
        let v = new_val.get_untracked();
        if k.trim().is_empty() {
            return;
        }
        set_rows.update(|r| r.push((k, v)));
        set_new_key.set(String::new());
        set_new_val.set(String::new());
    };

    let remove_row = move |idx: usize| {
        set_rows.update(|r| {
            r.remove(idx);
        });
    };

    let do_start = move |_| {
        // Flush any pending key/value the user typed but didn't explicitly "+ Add"
        add_row();

        let base_id = def_id.get_untracked();
        let id = match modal_version.get_untracked() {
            Some(v) => format!("{}:{}", base_id, v),
            None => base_id,
        };
        let bk = business_key.get_untracked();
        let variables: HashMap<String, serde_json::Value> = rows
            .get_untracked()
            .into_iter()
            .map(|(k, v)| (k, parse_value(&v)))
            .collect();

        set_submitting.set(true);
        set_error.set(None);
        spawn_local(async move {
            match api::start_instance(StartInstanceRequest {
                process_definition_id: id,
                business_key: if bk.trim().is_empty() { None } else { Some(bk) },
                variables,
            })
            .await
            {
                Ok(_) => {
                    on_started.call(());
                    on_close.call(());
                }
                Err(e) => {
                    set_error.set(Some(e));
                    set_submitting.set(false);
                }
            }
        });
    };

    view! {
        // Backdrop
        <div
            class="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
            on:click=move |ev| {
                if ev.target() == ev.current_target() { on_close.call(()); }
            }
        >
            <div class="bg-white dark:bg-gray-900 rounded-xl shadow-xl w-full max-w-lg mx-4 p-6">
                <h2 class="text-lg font-semibold text-gray-900 dark:text-gray-100 mb-4">
                    "Start Instance"
                </h2>

                // ── Version selector ─────────────────────────────────────
                {move || versions.get().map(|vd| {
                    view! {
                        <div class="mb-4">
                            <label class="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">
                                "Version"
                            </label>
                            <select
                                class="w-full text-xs px-2 py-1.5 rounded border border-gray-300 \
                                       dark:border-gray-700 bg-white dark:bg-gray-900 \
                                       text-gray-700 dark:text-gray-300 \
                                       focus:outline-none focus:ring-2 focus:ring-indigo-500 cursor-pointer"
                                prop:value=move || modal_version.get().map(|v| v.to_string()).unwrap_or_default()
                                on:change=move |ev| {
                                    set_modal_version.set(event_target_value(&ev).parse::<i32>().ok());
                                }
                            >
                                {vd.versions.iter().map(|&v| {
                                    let label = if v == vd.latest {
                                        format!("v{v} (latest)")
                                    } else {
                                        format!("v{v}")
                                    };
                                    let is_selected = modal_version.get_untracked() == Some(v);
                                    view! {
                                        <option value=v.to_string() selected=is_selected>{label}</option>
                                    }
                                }).collect_view()}
                            </select>
                        </div>
                    }
                })}

                // ── Business key ─────────────────────────────────────────
                <div class="mb-4">
                    <label class="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">
                        "Business Key "
                        <span class="font-normal text-gray-400 dark:text-gray-500">"(optional)"</span>
                    </label>
                    <input
                        type="text"
                        class="w-full font-mono text-xs px-2 py-1.5 border border-gray-300 \
                               dark:border-gray-700 rounded bg-white dark:bg-gray-900 \
                               text-gray-900 dark:text-gray-100 focus:outline-none \
                               focus:ring-2 focus:ring-indigo-500"
                        placeholder="e.g. order-123"
                        prop:value=move || business_key.get()
                        on:input=move |ev| set_business_key.set(event_target_value(&ev))
                    />
                </div>

                // ── Variable rows ────────────────────────────────────────
                <div class="mb-3 space-y-2">
                    {move || rows.get().into_iter().enumerate().map(|(i, (k, v))| {
                        view! {
                            <div class="flex items-center gap-2">
                                <input
                                    type="text"
                                    class="flex-1 font-mono text-xs px-2 py-1.5 border border-gray-300 \
                                           dark:border-gray-700 rounded bg-white dark:bg-gray-900 \
                                           text-gray-900 dark:text-gray-100 focus:outline-none \
                                           focus:ring-2 focus:ring-indigo-500"
                                    placeholder="key"
                                    prop:value=k
                                    readonly
                                />
                                <input
                                    type="text"
                                    class="flex-1 font-mono text-xs px-2 py-1.5 border border-gray-300 \
                                           dark:border-gray-700 rounded bg-white dark:bg-gray-900 \
                                           text-gray-900 dark:text-gray-100 focus:outline-none \
                                           focus:ring-2 focus:ring-indigo-500"
                                    placeholder="value"
                                    prop:value=v
                                    readonly
                                />
                                <button
                                    on:click=move |_| remove_row(i)
                                    class="text-gray-400 hover:text-red-500 transition-colors cursor-pointer"
                                    title="Remove"
                                >"×"</button>
                            </div>
                        }
                    }).collect_view()}
                </div>

                // ── Add row ──────────────────────────────────────────────
                <div class="flex items-center gap-2 mb-4">
                    <input
                        type="text"
                        class="flex-1 font-mono text-xs px-2 py-1.5 border border-gray-300 \
                               dark:border-gray-700 rounded bg-white dark:bg-gray-900 \
                               text-gray-900 dark:text-gray-100 focus:outline-none \
                               focus:ring-2 focus:ring-indigo-500"
                        placeholder="key"
                        prop:value=move || new_key.get()
                        on:input=move |ev| set_new_key.set(event_target_value(&ev))
                        on:keydown=move |ev| {
                            if ev.key() == "Enter" { add_row(); }
                        }
                    />
                    <input
                        type="text"
                        class="flex-1 font-mono text-xs px-2 py-1.5 border border-gray-300 \
                               dark:border-gray-700 rounded bg-white dark:bg-gray-900 \
                               text-gray-900 dark:text-gray-100 focus:outline-none \
                               focus:ring-2 focus:ring-indigo-500"
                        placeholder="value"
                        prop:value=move || new_val.get()
                        on:input=move |ev| set_new_val.set(event_target_value(&ev))
                        on:keydown=move |ev| {
                            if ev.key() == "Enter" { add_row(); }
                        }
                    />
                    <button
                        on:click=move |_| add_row()
                        disabled=move || new_key.get().trim().is_empty()
                        class="px-2 py-1.5 text-xs rounded bg-gray-100 dark:bg-gray-800 \
                               text-gray-600 dark:text-gray-300 hover:bg-gray-200 \
                               dark:hover:bg-gray-700 disabled:opacity-40 cursor-pointer"
                    >"+ Add"</button>
                </div>

                // ── Error ────────────────────────────────────────────────
                {move || error.get().map(|e| view! {
                    <p class="mb-3 text-xs text-red-500">{e}</p>
                })}

                // ── Footer ───────────────────────────────────────────────
                <div class="flex justify-end gap-2">
                    <button
                        on:click=move |_| on_close.call(())
                        class="px-4 py-2 text-sm rounded-md border border-gray-300 dark:border-gray-700 \
                               text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-800 \
                               cursor-pointer"
                    >"Cancel"</button>
                    <button
                        on:click=do_start
                        disabled=move || submitting.get()
                        class="px-4 py-2 text-sm font-medium rounded-md bg-indigo-600 text-white \
                               hover:bg-indigo-700 disabled:opacity-50 cursor-pointer transition-colors"
                    >
                        {move || if submitting.get() { "Starting…" } else { "Start" }}
                    </button>
                </div>
            </div>
        </div>
    }
}
