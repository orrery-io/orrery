use leptos::*;
use orrery_types::ProcessInstanceResponse;

#[component]
pub fn VariablesTab(
    // The parent's instance_data signal — may update while we're editing.
    instance_data: ReadSignal<Option<ProcessInstanceResponse>>,
    // Called after a successful save so the parent can refetch.
    on_save: Callback<()>,
    set_action_status: WriteSignal<Option<(bool, String)>>,
) -> impl IntoView {
    // ── Editing state (owned here, not in the parent) ──────────────────────
    // Two-signal pattern: outer reactive closures subscribe to editing_key
    // (changes only when entering/leaving edit mode), not editing_value
    // (changes on every keystroke). This prevents input recreation on keypress.
    let (editing_key, set_editing_key) = create_signal(Option::<String>::None);
    let (editing_value, set_editing_value) = create_signal(String::new());
    let (adding_var, set_adding_var) = create_signal(false);
    let (new_var_key, set_new_var_key) = create_signal(String::new());
    let (new_var_value, set_new_var_value) = create_signal(String::new());
    let (var_saving, set_var_saving) = create_signal(false);

    // ── Local anti-flicker guard ───────────────────────────────────────────
    // Keep a local copy of instance_data that is NOT updated while the user
    // is in the middle of editing or adding a variable. This prevents the
    // polling-driven re-render from destroying focused inputs.
    let (local_data, set_local_data) = create_signal(Option::<ProcessInstanceResponse>::None);

    create_effect(move |_| {
        if let Some(inst) = instance_data.get() {
            // Suppress update while any editing is active.
            let editing = editing_key.get_untracked().is_some() || adding_var.get_untracked();
            if !editing {
                set_local_data.set(Some(inst));
            }
        }
    });

    // ── Save helper — shared by button click and Enter keydown ────────────
    // Returns a serde_json::Value by parsing a raw string the same way the
    // original code did (bool > i64 > f64 > String).
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

    let do_save_existing = move |iid: String, key: String, raw: String| {
        let mut patch_map = serde_json::Map::new();
        patch_map.insert(key, parse_value(&raw));
        let patch = serde_json::Value::Object(patch_map);
        set_var_saving.set(true);
        set_action_status.set(None);
        spawn_local(async move {
            match crate::api::update_instance_variables(&iid, patch).await {
                Ok(_) => {
                    set_editing_key.set(None);
                    on_save.call(());
                }
                Err(e) => set_action_status.set(Some((false, e))),
            }
            set_var_saving.set(false);
        });
    };

    let do_save_new = move |iid: String, key: String, raw: String| {
        if key.is_empty() {
            return;
        }
        let mut patch_map = serde_json::Map::new();
        patch_map.insert(key, parse_value(&raw));
        let patch = serde_json::Value::Object(patch_map);
        set_var_saving.set(true);
        set_action_status.set(None);
        spawn_local(async move {
            match crate::api::update_instance_variables(&iid, patch).await {
                Ok(_) => {
                    set_adding_var.set(false);
                    set_new_var_key.set(String::new());
                    set_new_var_value.set(String::new());
                    on_save.call(());
                }
                Err(e) => set_action_status.set(Some((false, e))),
            }
            set_var_saving.set(false);
        });
    };

    view! {
        {move || {
            let inst_opt = local_data.get();
            let is_editable = inst_opt.as_ref()
                .map(|i| !matches!(i.state.as_str(), "COMPLETED" | "CANCELLED"))
                .unwrap_or(false);
            let inst_id = inst_opt.as_ref().map(|i| i.id.clone()).unwrap_or_default();
            let has_data = inst_opt.is_some();

            let table_view: leptos::View = match inst_opt.and_then(|i| i.variables.as_object().cloned()) {
                None => view! {
                    <p class="p-4 text-xs text-gray-400 italic">"Loading…"</p>
                }.into_view(),
                Some(ref m) if m.is_empty() => view! {
                    <p class="p-4 text-xs text-gray-400 italic">"No variables."</p>
                }.into_view(),
                Some(map) => view! {
                    <table class="w-full text-xs">
                        <thead>
                            <tr class="border-b border-gray-100 dark:border-gray-900">
                                <th class="text-left py-1.5 px-4 font-medium text-gray-500">"Key"</th>
                                <th class="text-left py-1.5 px-4 font-medium text-gray-500">"Type"</th>
                                <th class="text-left py-1.5 px-4 font-medium text-gray-500">"Value"</th>
                            </tr>
                        </thead>
                        <tbody>
                            {map.into_iter()
                                .filter(|(k, _)| !k.starts_with("__"))
                                .map(|(key, val)| {
                                    let (type_label, val_display) = match &val {
                                        serde_json::Value::String(s) => ("string", format!("\"{}\"", s)),
                                        serde_json::Value::Bool(b)   => ("bool", b.to_string()),
                                        serde_json::Value::Number(n)  => ("number", n.to_string()),
                                        serde_json::Value::Null      => ("null", "null".to_string()),
                                        other                        => ("object", other.to_string()),
                                    };
                                    let is_bool    = matches!(&val, serde_json::Value::Bool(_));
                                    let bool_true  = matches!(&val, serde_json::Value::Bool(true));
                                    let is_null    = matches!(&val, serde_json::Value::Null);
                                    let is_complex = matches!(&val, serde_json::Value::Object(_) | serde_json::Value::Array(_));
                                    let raw_init   = match &val {
                                        serde_json::Value::String(s) => s.clone(),
                                        serde_json::Value::Bool(b)   => b.to_string(),
                                        serde_json::Value::Number(n)  => n.to_string(),
                                        _                            => val.to_string(),
                                    };
                                    let key_td    = key.clone();
                                    let inst_id_td = inst_id.clone();
                                    view! {
                                        <tr class="border-b border-gray-100 dark:border-gray-900">
                                            <td class="py-1.5 px-4 font-mono text-gray-700 dark:text-gray-300">{key}</td>
                                            <td class="py-1.5 px-4 text-gray-400">{type_label}</td>
                                            <td class="py-1.5 px-4">
                                                {move || {
                                                    let k   = key_td.clone();
                                                    let iid = inst_id_td.clone();
                                                    let is_editing_this = editing_key.get()
                                                        .as_deref() == Some(k.as_str());
                                                    if is_editing_this {
                                                        let k_save  = k.clone();
                                                        let iid_save = iid.clone();
                                                        let k_kd    = k.clone();
                                                        let iid_kd  = iid.clone();
                                                        view! {
                                                            <div class="flex items-center gap-1">
                                                                {if is_bool {
                                                                    view! {
                                                                        <select
                                                                            class="border border-indigo-300 dark:border-indigo-700 \
                                                                                   rounded px-1.5 py-0.5 text-xs font-mono \
                                                                                   bg-white dark:bg-gray-900 \
                                                                                   text-gray-800 dark:text-gray-200"
                                                                            on:change=move |e| set_editing_value.set(event_target_value(&e))
                                                                            on:keydown=move |e| {
                                                                                if e.key() == "Escape" { set_editing_key.set(None); }
                                                                            }
                                                                        >
                                                                            <option value="true"
                                                                                selected=move || {
                                                                                    let v = editing_value.get();
                                                                                    if v.is_empty() { bool_true } else { v == "true" }
                                                                                }
                                                                            >"true"</option>
                                                                            <option value="false"
                                                                                selected=move || {
                                                                                    let v = editing_value.get();
                                                                                    if v.is_empty() { !bool_true } else { v == "false" }
                                                                                }
                                                                            >"false"</option>
                                                                        </select>
                                                                    }.into_view()
                                                                } else {
                                                                    view! {
                                                                        <input
                                                                            type="text"
                                                                            class=if is_complex {
                                                                                "border border-indigo-300 dark:border-indigo-700 \
                                                                                 rounded px-1.5 py-0.5 text-xs font-mono w-56 \
                                                                                 bg-white dark:bg-gray-900 \
                                                                                 text-gray-800 dark:text-gray-200"
                                                                            } else {
                                                                                "border border-indigo-300 dark:border-indigo-700 \
                                                                                 rounded px-1.5 py-0.5 text-xs font-mono w-32 \
                                                                                 bg-white dark:bg-gray-900 \
                                                                                 text-gray-800 dark:text-gray-200"
                                                                            }
                                                                            prop:value=move || editing_value.get()
                                                                            on:input=move |e| set_editing_value.set(event_target_value(&e))
                                                                            on:keydown=move |e| {
                                                                                match e.key().as_str() {
                                                                                    "Enter" => {
                                                                                        let raw = editing_value.get_untracked();
                                                                                        do_save_existing(iid_kd.clone(), k_kd.clone(), raw);
                                                                                    }
                                                                                    "Escape" => set_editing_key.set(None),
                                                                                    _ => {}
                                                                                }
                                                                            }
                                                                        />
                                                                    }.into_view()
                                                                }}
                                                                <button
                                                                    disabled=move || var_saving.get()
                                                                    on:click=move |_| {
                                                                        let raw = editing_value.get_untracked();
                                                                        do_save_existing(iid_save.clone(), k_save.clone(), raw);
                                                                    }
                                                                    class="px-1.5 py-0.5 text-xs rounded \
                                                                           bg-indigo-600 text-white \
                                                                           hover:bg-indigo-700 \
                                                                           disabled:opacity-50 transition-colors cursor-pointer"
                                                                >"Save"</button>
                                                                <button
                                                                    on:click=move |_| set_editing_key.set(None)
                                                                    class="px-1.5 py-0.5 text-xs rounded border \
                                                                           border-gray-300 dark:border-gray-600 \
                                                                           text-gray-600 dark:text-gray-400 \
                                                                           hover:bg-gray-50 dark:hover:bg-gray-800 cursor-pointer"
                                                                >"✕"</button>
                                                            </div>
                                                        }.into_view()
                                                    } else {
                                                        let val_part: leptos::View = if is_bool {
                                                            let cls = if bool_true {
                                                                "text-emerald-600 dark:text-emerald-400"
                                                            } else {
                                                                "text-red-600 dark:text-red-400"
                                                            };
                                                            view! { <span class=cls>{val_display.clone()}</span> }.into_view()
                                                        } else if is_null {
                                                            view! { <span class="text-gray-400 italic">"null"</span> }.into_view()
                                                        } else {
                                                            view! { <span class="font-mono text-gray-700 dark:text-gray-300">{val_display.clone()}</span> }.into_view()
                                                        };
                                                        if is_editable && !is_null {
                                                            let pencil_k   = k.clone();
                                                            let pencil_raw = raw_init.clone();
                                                            view! {
                                                                <div class="flex items-center gap-2 group">
                                                                    {val_part}
                                                                    <button
                                                                        on:click=move |_| {
                                                                            set_editing_key.set(Some(pencil_k.clone()));
                                                                            set_editing_value.set(pencil_raw.clone());
                                                                        }
                                                                        class="opacity-0 group-hover:opacity-100 \
                                                                               text-gray-400 hover:text-gray-600 \
                                                                               dark:hover:text-gray-300 \
                                                                               text-xs transition-opacity leading-none cursor-pointer"
                                                                        title="Edit"
                                                                    >"✎"</button>
                                                                </div>
                                                            }.into_view()
                                                        } else {
                                                            val_part
                                                        }
                                                    }
                                                }}
                                            </td>
                                        </tr>
                                    }
                                }).collect_view()}
                        </tbody>
                    </table>
                }.into_view(),
            };

            view! {
                <div>
                    {table_view}
                    {(is_editable && has_data).then(|| {
                        let iid_add = inst_id.clone();
                        view! {
                            <div class="px-4 py-2 border-t border-gray-100 dark:border-gray-900">
                                {move || if adding_var.get() {
                                    let iid    = iid_add.clone();
                                    let iid_kd = iid.clone();
                                    view! {
                                        <div class="flex items-center gap-2 flex-wrap">
                                            <input
                                                type="text"
                                                placeholder="key"
                                                class="border border-indigo-300 dark:border-indigo-700 \
                                                       rounded px-1.5 py-0.5 text-xs font-mono w-28 \
                                                       bg-white dark:bg-gray-900 \
                                                       text-gray-800 dark:text-gray-200"
                                                prop:value=move || new_var_key.get()
                                                on:input=move |e| set_new_var_key.set(event_target_value(&e))
                                                on:keydown=move |e| {
                                                    if e.key() == "Escape" {
                                                        set_adding_var.set(false);
                                                        set_new_var_key.set(String::new());
                                                        set_new_var_value.set(String::new());
                                                    }
                                                }
                                            />
                                            <input
                                                type="text"
                                                placeholder="value"
                                                class="border border-indigo-300 dark:border-indigo-700 \
                                                       rounded px-1.5 py-0.5 text-xs font-mono w-32 \
                                                       bg-white dark:bg-gray-900 \
                                                       text-gray-800 dark:text-gray-200"
                                                prop:value=move || new_var_value.get()
                                                on:input=move |e| set_new_var_value.set(event_target_value(&e))
                                                on:keydown=move |e| {
                                                    match e.key().as_str() {
                                                        "Enter" => {
                                                            let key = new_var_key.get_untracked();
                                                            let raw = new_var_value.get_untracked();
                                                            do_save_new(iid_kd.clone(), key, raw);
                                                        }
                                                        "Escape" => {
                                                            set_adding_var.set(false);
                                                            set_new_var_key.set(String::new());
                                                            set_new_var_value.set(String::new());
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                            />
                                            <button
                                                disabled=move || var_saving.get()
                                                on:click=move |_| {
                                                    let key = new_var_key.get_untracked();
                                                    let raw = new_var_value.get_untracked();
                                                    do_save_new(iid.clone(), key, raw);
                                                }
                                                class="px-1.5 py-0.5 text-xs rounded \
                                                       bg-indigo-600 text-white hover:bg-indigo-700 \
                                                       disabled:opacity-50 transition-colors cursor-pointer"
                                            >"Add"</button>
                                            <button
                                                on:click=move |_| {
                                                    set_adding_var.set(false);
                                                    set_new_var_key.set(String::new());
                                                    set_new_var_value.set(String::new());
                                                }
                                                class="px-1.5 py-0.5 text-xs rounded border \
                                                       border-gray-300 dark:border-gray-600 \
                                                       text-gray-600 dark:text-gray-400 \
                                                       hover:bg-gray-50 dark:hover:bg-gray-800 cursor-pointer"
                                            >"✕"</button>
                                        </div>
                                    }.into_view()
                                } else {
                                    view! {
                                        <button
                                            on:click=move |_| set_adding_var.set(true)
                                            class="text-xs text-indigo-500 hover:text-indigo-700 \
                                                   dark:text-indigo-400 dark:hover:text-indigo-300 \
                                                   transition-colors cursor-pointer"
                                        >"+ Add variable"</button>
                                    }.into_view()
                                }}
                            </div>
                        }
                    })}
                </div>
            }.into_view()
        }}
    }
}
