use leptos::*;
use leptos_router::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{DragEvent, HtmlInputElement};

use crate::api;
use crate::components::empty_state::EmptyState;
use crate::components::status_badge::relative_time;

#[derive(Clone, PartialEq)]
enum UploadTab {
    Drop,
    Paste,
}

#[component]
pub fn DefinitionsPage() -> impl IntoView {
    let (tab, set_tab) = create_signal(UploadTab::Drop);
    let (bpmn_input, set_bpmn_input) = create_signal(String::new());
    let (drag_over, set_drag_over) = create_signal(false);
    let (file_name, set_file_name) = create_signal(Option::<String>::None);
    let (deploying, set_deploying) = create_signal(false);
    let (deploy_error, set_deploy_error) = create_signal(Option::<String>::None);
    let (deploy_success, set_deploy_success) = create_signal(Option::<String>::None);
    let (modal_open, set_modal_open) = create_signal(false);

    let definitions = create_resource(|| (), |_| async { api::list_definitions().await });

    let close_modal = move || {
        set_modal_open.set(false);
        set_deploy_error.set(None);
        set_bpmn_input.set(String::new());
        set_file_name.set(None);
        set_tab.set(UploadTab::Drop);
    };

    let deploy = move |_| {
        let xml = bpmn_input.get();
        if xml.trim().is_empty() {
            set_deploy_error.set(Some("No BPMN content to deploy.".to_string()));
            return;
        }
        set_deploying.set(true);
        set_deploy_error.set(None);
        spawn_local(async move {
            match api::deploy_definition(xml).await {
                Ok(def) => {
                    set_deploy_success.set(Some(format!("Deployed version {}", def.version)));
                    set_bpmn_input.set(String::new());
                    set_file_name.set(None);
                    set_modal_open.set(false);
                    definitions.refetch();
                }
                Err(e) => set_deploy_error.set(Some(e)),
            }
            set_deploying.set(false);
        });
    };

    let load_file = move |file: web_sys::File| {
        let name = file.name();
        if !name.ends_with(".bpmn") && !name.ends_with(".xml") {
            set_deploy_error.set(Some("Only .bpmn or .xml files accepted.".to_string()));
            return;
        }
        set_file_name.set(Some(name));
        set_deploy_error.set(None);
        let reader = web_sys::FileReader::new().unwrap();
        let reader_clone = reader.clone();
        let closure = Closure::wrap(Box::new(move |_: web_sys::Event| {
            if let Ok(result) = reader_clone.result() {
                if let Some(text) = result.as_string() {
                    set_bpmn_input.set(text);
                }
            }
        }) as Box<dyn FnMut(_)>);
        reader.set_onload(Some(closure.as_ref().unchecked_ref()));
        closure.forget();
        let _ = reader.read_as_text(&file);
    };

    let on_drop = move |ev: DragEvent| {
        ev.prevent_default();
        set_drag_over.set(false);
        if let Some(dt) = ev.data_transfer() {
            if let Some(files) = dt.files() {
                if files.length() > 0 {
                    if let Some(file) = files.get(0) {
                        load_file(file);
                    }
                }
            }
        }
    };

    let on_click_browse = move |_| {
        let document = web_sys::window().unwrap().document().unwrap();
        if let Some(el) = document.get_element_by_id("bpmn-file-input") {
            el.unchecked_into::<HtmlInputElement>().click();
        }
    };

    let on_file_input = move |ev: web_sys::Event| {
        let input = ev.target().unwrap().unchecked_into::<HtmlInputElement>();
        if let Some(files) = input.files() {
            if files.length() > 0 {
                if let Some(file) = files.get(0) {
                    load_file(file);
                }
            }
        }
    };

    view! {
        <div class="p-6">
            // ── Page header ──────────────────────────────────────────────────
            <div class="flex items-center justify-between mb-6">
                <h1 class="text-xl font-semibold text-gray-900 dark:text-gray-100">"Processes"</h1>
                <button
                    class="px-4 py-2 text-sm font-medium rounded-md bg-indigo-600 text-white hover:bg-indigo-700 transition-colors cursor-pointer"
                    on:click=move |_| set_modal_open.set(true)
                >
                    "Deploy"
                </button>
            </div>

            // ── Success toast ────────────────────────────────────────────────
            {move || deploy_success.get().map(|s| view! {
                <div class="mb-4 px-4 py-2 bg-emerald-50 dark:bg-emerald-950/30 border border-emerald-200 dark:border-emerald-800 rounded text-sm text-emerald-700 dark:text-emerald-400">
                    "✓ "{s}
                </div>
            })}

            // ── Definitions table ────────────────────────────────────────────
            <Suspense fallback=|| view! {
                <p class="text-sm text-gray-500">"Loading…"</p>
            }>
                {move || definitions.get().map(|result| match result {
                    Err(e) => view! {
                        <p class="text-sm text-red-500">{format!("Error: {e}")}</p>
                    }.into_view(),
                    Ok(list) if list.items.is_empty() => view! {
                        <EmptyState
                            title="No processes deployed yet"
                            subtitle="Click Deploy to upload a BPMN file."
                        />
                    }.into_view(),
                    Ok(list) => view! {
                        <table class="w-full text-sm border-collapse">
                            <thead>
                                <tr class="border-b border-gray-200 dark:border-gray-800">
                                    <th class="text-left py-2 px-3 text-xs font-medium text-gray-500 dark:text-gray-400">"Definition ID"</th>
                                    <th class="text-left py-2 px-3 text-xs font-medium text-gray-500 dark:text-gray-400">"Version"</th>
                                    <th class="text-right py-2 px-3 text-xs font-medium text-blue-500">"Running"</th>
                                    <th class="text-right py-2 px-3 text-xs font-medium text-emerald-500">"Completed"</th>
                                    <th class="text-right py-2 px-3 text-xs font-medium text-red-500">"Failed"</th>
                                    <th class="text-left py-2 px-3 text-xs font-medium text-gray-500 dark:text-gray-400">"Deployed"</th>
                                    <th class="py-2 px-3"></th>
                                </tr>
                            </thead>
                            <tbody>
                                {list.items.into_iter().map(|def| {
                                    let instances_href = format!("/definitions/{}/instances", def.id);
                                    let age = relative_time(&def.created_at);
                                    let full_id = def.id.clone();
                                    let running = def.running_count;
                                    let completed = def.completed_count;
                                    let failed = def.failed_count;
                                    view! {
                                        <tr class="border-b border-gray-100 dark:border-gray-900 hover:bg-gray-50 dark:hover:bg-gray-900/50">
                                            <td class="py-2.5 px-3">
                                                <A
                                                    href=instances_href.clone()
                                                    class="font-mono text-xs text-indigo-600 dark:text-indigo-400 hover:underline break-all"
                                                >
                                                    {full_id}
                                                </A>
                                            </td>
                                            <td class="py-2.5 px-3 text-xs text-gray-500">{def.version}</td>
                                            <td class="py-2.5 px-3 text-right text-xs font-medium text-blue-600 dark:text-blue-400">
                                                {move || if running > 0 { running.to_string() } else { "—".to_string() }}
                                            </td>
                                            <td class="py-2.5 px-3 text-right text-xs font-medium text-emerald-600 dark:text-emerald-400">
                                                {move || if completed > 0 { completed.to_string() } else { "—".to_string() }}
                                            </td>
                                            <td class="py-2.5 px-3 text-right text-xs font-medium text-red-600 dark:text-red-400">
                                                {move || if failed > 0 { failed.to_string() } else { "—".to_string() }}
                                            </td>
                                            <td class="py-2.5 px-3 text-xs text-gray-500">{age}</td>
                                            <td class="py-2.5 px-3 text-right">
                                                <A
                                                    href=instances_href
                                                    class="text-xs text-gray-400 dark:text-gray-500 hover:text-indigo-600 dark:hover:text-indigo-400"
                                                >
                                                    "→"
                                                </A>
                                            </td>
                                        </tr>
                                    }
                                }).collect_view()}
                            </tbody>
                        </table>
                    }.into_view(),
                })}
            </Suspense>
        </div>

        // ── Deploy Modal ─────────────────────────────────────────────────────
        {move || modal_open.get().then(|| view! {
            <div
                class="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
                on:click=move |ev| {
                    if ev.target() == ev.current_target() {
                        close_modal();
                    }
                }
            >
                <div class="bg-white dark:bg-gray-900 rounded-xl shadow-xl w-full max-w-lg mx-4 p-6">
                    <h2 class="text-lg font-semibold text-gray-900 dark:text-gray-100 mb-4">
                        "Deploy Process Definition"
                    </h2>

                    // Tab switcher
                    <div class="flex gap-1 mb-3">
                        <button
                            class=move || if tab.get() == UploadTab::Drop {
                                "px-3 py-1.5 text-sm rounded bg-indigo-50 dark:bg-indigo-950 text-indigo-700 dark:text-indigo-300 font-medium cursor-pointer"
                            } else {
                                "px-3 py-1.5 text-sm rounded text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800 cursor-pointer"
                            }
                            on:click=move |_| set_tab.set(UploadTab::Drop)
                        >"Drop File"</button>
                        <button
                            class=move || if tab.get() == UploadTab::Paste {
                                "px-3 py-1.5 text-sm rounded bg-indigo-50 dark:bg-indigo-950 text-indigo-700 dark:text-indigo-300 font-medium cursor-pointer"
                            } else {
                                "px-3 py-1.5 text-sm rounded text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800 cursor-pointer"
                            }
                            on:click=move |_| set_tab.set(UploadTab::Paste)
                        >"Paste XML"</button>
                    </div>

                    {move || if tab.get() == UploadTab::Drop {
                        let dropzone_class = move || {
                            let base = "border-2 border-dashed rounded-lg min-h-[140px] flex flex-col items-center justify-center gap-2 cursor-pointer transition-colors p-6";
                            if drag_over.get() {
                                format!("{base} border-indigo-500 bg-indigo-50 dark:bg-indigo-950/30")
                            } else if file_name.get().is_some() {
                                format!("{base} border-emerald-500 bg-emerald-50 dark:bg-emerald-950/20")
                            } else {
                                format!("{base} border-gray-300 dark:border-gray-600 hover:border-gray-400 dark:hover:border-gray-500")
                            }
                        };
                        view! {
                            <div>
                                <input
                                    id="bpmn-file-input"
                                    type="file"
                                    accept=".bpmn,.xml"
                                    class="hidden"
                                    on:change=on_file_input
                                />
                                <div
                                    class=dropzone_class
                                    on:dragover=move |ev: DragEvent| { ev.prevent_default(); set_drag_over.set(true); }
                                    on:dragleave=move |_| set_drag_over.set(false)
                                    on:drop=on_drop
                                    on:click=on_click_browse
                                >
                                    {move || if let Some(name) = file_name.get() {
                                        view! {
                                            <span class="text-2xl">"✓"</span>
                                            <span class="text-sm font-medium text-emerald-700 dark:text-emerald-400">{name}</span>
                                            <span class="text-xs text-gray-500">"Click to replace"</span>
                                        }.into_view()
                                    } else {
                                        view! {
                                            <span class="text-2xl text-gray-400">"↑"</span>
                                            <span class="text-sm text-gray-600 dark:text-gray-400">"Drop .bpmn or .xml file here"</span>
                                            <span class="text-xs text-gray-400">"or click to browse"</span>
                                        }.into_view()
                                    }}
                                </div>
                            </div>
                        }.into_view()
                    } else {
                        view! {
                            <textarea
                                rows="7"
                                placeholder="Paste BPMN 2.0 XML here…"
                                class="w-full font-mono text-xs p-3 border border-gray-300 dark:border-gray-700 rounded-lg bg-white dark:bg-gray-900 text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-indigo-500 resize-y"
                                on:input=move |ev| set_bpmn_input.set(event_target_value(&ev))
                                prop:value=move || bpmn_input.get()
                            />
                        }.into_view()
                    }}

                    {move || deploy_error.get().map(|e| view! {
                        <p class="mt-2 text-sm text-red-600 dark:text-red-400">{e}</p>
                    })}

                    // Modal footer
                    <div class="mt-4 flex justify-end gap-2">
                        <button
                            class="px-4 py-2 text-sm rounded-md border border-gray-300 dark:border-gray-700 text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-800 cursor-pointer"
                            on:click=move |_| close_modal()
                        >"Cancel"</button>
                        <button
                            on:click=deploy
                            disabled=move || deploying.get() || bpmn_input.get().trim().is_empty()
                            class="px-4 py-2 text-sm font-medium rounded-md bg-indigo-600 text-white hover:bg-indigo-700 disabled:opacity-50 disabled:cursor-not-allowed cursor-pointer transition-colors"
                        >
                            {move || if deploying.get() { "Deploying…" } else { "Deploy" }}
                        </button>
                    </div>
                </div>
            </div>
        })}
    }
}
