use leptos::*;
use leptos_router::*;

mod start_modal;
use start_modal::StartModal;

use orrery_types::ProcessDefinitionVersionsResponse;

use crate::api;
use crate::components::bpmn_viewer::BpmnViewer;
use crate::components::instances_table::InstancesTable;
use crate::components::page_breadcrumb::PageBreadcrumb;

#[derive(Clone, PartialEq)]
enum InstanceFilter {
    All,
    Running,
    Waiting,
    Completed,
    Failed,
    Cancelled,
}

impl InstanceFilter {
    fn label(&self) -> &'static str {
        match self {
            InstanceFilter::All => "All",
            InstanceFilter::Running => "Running",
            InstanceFilter::Waiting => "Waiting",
            InstanceFilter::Completed => "Completed",
            InstanceFilter::Failed => "Failed",
            InstanceFilter::Cancelled => "Cancelled",
        }
    }
    fn state_param(&self) -> Option<&'static str> {
        match self {
            InstanceFilter::All => None,
            InstanceFilter::Running => Some("RUNNING"),
            InstanceFilter::Waiting => Some("WAITING_FOR_TASK"),
            InstanceFilter::Completed => Some("COMPLETED"),
            InstanceFilter::Failed => Some("FAILED"),
            InstanceFilter::Cancelled => Some("CANCELLED"),
        }
    }
}

#[component]
pub fn DefinitionInstancesPage() -> impl IntoView {
    let params = use_params_map();
    let def_id = move || params.with(|p| p.get("id").cloned().unwrap_or_default());

    let (filter, set_filter) = create_signal(InstanceFilter::All);
    let (show_start_modal, set_show_start_modal) = create_signal(false);
    let (activity_filter, set_activity_filter) = create_signal(Option::<String>::None);
    let (search, set_search) = create_signal(String::new());

    // ── URL query params ──────────────────────────────────────────────────────
    let query = use_query_map();
    let version_param =
        move || query.with(|q| q.get("version").and_then(|v| v.parse::<i32>().ok()));
    let page_param = move || {
        query.with(|q| {
            q.get("page")
                .and_then(|p| p.parse::<u32>().ok())
                .unwrap_or(1)
        })
    };

    // ── Navigate (called at component scope so it's safe to capture) ──────────
    let navigate = use_navigate();

    // ── Definition metadata ───────────────────────────────────────────────────
    let definition = create_resource(def_id, |id| async move {
        api::list_definitions()
            .await
            .map(|r| r.items.into_iter().find(|d| d.id == id))
    });

    // ── Versions resource + auto-navigate to latest ───────────────────────────
    let versions_resource = create_resource(def_id, |id| async move {
        api::list_definition_versions(&id).await
    });

    let (versions_data, set_versions_data) =
        create_signal(Option::<ProcessDefinitionVersionsResponse>::None);

    create_effect(move |_| {
        if let Some(Ok(vd)) = versions_resource.get() {
            set_versions_data.set(Some(vd.clone()));
            // On first load, if no version in URL yet, redirect to latest (replace history)
            if version_param().is_none() {
                navigate(
                    &format!(
                        "/definitions/{}/instances?version={}&page=1",
                        def_id(),
                        vd.latest
                    ),
                    NavigateOptions {
                        replace: true,
                        ..Default::default()
                    },
                );
            }
        }
    });

    // ── Instances resource (version + page aware) ─────────────────────────────
    let instances = create_resource(
        move || {
            (
                def_id(),
                filter.get().state_param().map(String::from),
                version_param(),
                page_param(),
            )
        },
        |(id, state, version, page)| async move {
            api::list_instances_for_definition(&id, state.as_deref(), version, Some(page), Some(20))
                .await
        },
    );

    // ── Anti-flicker signals ──────────────────────────────────────────────────
    let (def_data, set_def_data) =
        create_signal(Option::<orrery_types::ProcessDefinitionResponse>::None);
    let (instances_data, set_instances_data) =
        create_signal(Option::<Vec<orrery_types::ProcessInstanceResponse>>::None);
    let (pagination_data, set_pagination_data) = create_signal((1u32, 1u32)); // (current_page, total_pages)

    create_effect(move |_| {
        if let Some(Ok(Some(d))) = definition.get() {
            set_def_data.set(Some(d));
        }
    });
    create_effect(move |_| {
        if let Some(Ok(paginated)) = instances.get() {
            set_instances_data.set(Some(paginated.items));
            set_pagination_data.set((paginated.page, paginated.total_pages));
        }
    });

    // Pre-filter by BPMN activity selection before passing to the table component.
    // When a BPMN element is clicked, only instances touching that element are shown.
    let activity_filtered = Signal::derive(move || {
        instances_data.get().map(|list| {
            if let Some(ref eid) = activity_filter.get() {
                list.into_iter()
                    .filter(|inst| {
                        let in_active = inst
                            .active_element_ids
                            .as_array()
                            .map(|arr| arr.iter().any(|v| v.as_str() == Some(eid.as_str())))
                            .unwrap_or(false);
                        let in_failed = inst
                            .failed_at_element_id
                            .as_deref()
                            .map(|fid| fid == eid.as_str())
                            .unwrap_or(false);
                        in_active || in_failed
                    })
                    .collect()
            } else {
                list
            }
        })
    });

    // ── Pagination signals for InstancesTable ─────────────────────────────────
    let current_page_sig = Signal::derive(page_param);
    let total_pages_sig = Signal::derive(move || pagination_data.get().1);
    let nav_page = use_navigate();
    let on_page_change = Callback::new(move |p: u32| {
        let v = version_param().unwrap_or(1);
        nav_page(
            &format!(
                "/definitions/{}/instances?version={}&page={}",
                def_id(),
                v,
                p
            ),
            Default::default(),
        );
    });

    // Poll every 5s
    spawn_local(async move {
        loop {
            gloo_timers::future::sleep(std::time::Duration::from_secs(5)).await;
            instances.refetch();
            definition.refetch();
        }
    });

    let filters = [
        InstanceFilter::All,
        InstanceFilter::Running,
        InstanceFilter::Waiting,
        InstanceFilter::Completed,
        InstanceFilter::Failed,
        InstanceFilter::Cancelled,
    ];

    view! {
        <div class="p-6">
            // ── Header ───────────────────────────────────────────────────
            <PageBreadcrumb>
                {move || match def_data.get() {
                    None => view! { <span class="text-sm text-gray-400">{def_id()}</span> }.into_view(),
                    Some(d) => view! {
                        <span class="text-sm font-semibold text-gray-900 dark:text-gray-100">
                            {d.id.clone()}
                            ":"
                            {move || {
                                let cur = version_param();
                                let is_latest = versions_data.get()
                                    .map(|vd| cur == Some(vd.latest))
                                    .unwrap_or(false);
                                if is_latest {
                                    "latest".to_string()
                                } else {
                                    cur.map(|v| v.to_string()).unwrap_or_default()
                                }
                            }}
                        </span>
                        {(d.running_count > 0).then(|| view! {
                            <span class="text-xs text-blue-600 dark:text-blue-400">
                                "● " {d.running_count} " running"
                            </span>
                        })}
                        {(d.failed_count > 0).then(|| view! {
                            <span class="text-xs text-red-600 dark:text-red-400">
                                "⚠ " {d.failed_count} " failed"
                            </span>
                        })}
                    }.into_view(),
                }}
                <div class="ml-auto flex items-center gap-2">
                    // Version dropdown
                    {move || versions_data.get().map(|vd| {
                        let cur_ver = version_param();
                        let nav_ver = use_navigate();
                        view! {
                            <select
                                class="text-xs px-2 py-1.5 rounded border border-gray-200 dark:border-gray-700 \
                                       bg-white dark:bg-gray-900 text-gray-700 dark:text-gray-300 \
                                       focus:outline-none focus:ring-1 focus:ring-indigo-400 cursor-pointer"
                                prop:value=cur_ver.map(|v| v.to_string()).unwrap_or_default()
                                on:change=move |ev| {
                                    let v = event_target_value(&ev);
                                    nav_ver(
                                        &format!("/definitions/{}/instances?version={}&page=1", def_id(), v),
                                        Default::default(),
                                    );
                                }
                            >
                                {vd.versions.iter().map(|&v| {
                                    let label = if v == vd.latest {
                                        format!("v{v} (latest)")
                                    } else {
                                        format!("v{v}")
                                    };
                                    let is_selected = cur_ver == Some(v);
                                    view! {
                                        <option value=v.to_string() selected=is_selected>
                                            {label}
                                        </option>
                                    }
                                }).collect_view()}
                            </select>
                        }
                    })}
                    // Start Instance button
                    <button
                        on:click=move |_| set_show_start_modal.set(true)
                        class="px-3 py-1.5 text-sm font-medium rounded bg-indigo-600 text-white hover:bg-indigo-700 transition-colors cursor-pointer"
                    >
                        "Start Instance"
                    </button>
                </div>
            </PageBreadcrumb>

            // ── Aggregate diagram ─────────────────────────────────────────
            <div class="mb-6">
                <BpmnViewer
                    diagram_url=move || {
                        let base = format!("/v1/process-definitions/{}/diagram", def_id());
                        match version_param() {
                            Some(v) => format!("{}?version={}", base, v),
                            None    => base,
                        }
                    }
                    on_element_click=Callback::new(move |eid: String| {
                        set_activity_filter.update(|f| {
                            if f.as_deref() == Some(eid.as_str()) {
                                *f = None; // toggle off
                            } else {
                                *f = Some(eid);
                            }
                        });
                    })
                />
            </div>

            // ── Filter pills + search (same row) ─────────────────────────
            <div class="flex items-center gap-1 mb-3">
                {filters.into_iter().map(|f| {
                    let label = f.label();
                    let f_cmp = f.clone();
                    let f_click = f.clone();
                    view! {
                        <button
                            class=move || {
                                let base = "px-3 py-1 text-xs font-medium rounded-full transition-colors cursor-pointer";
                                if filter.get() == f_cmp {
                                    format!("{base} bg-indigo-100 dark:bg-indigo-900 text-indigo-700 dark:text-indigo-300")
                                } else {
                                    format!("{base} text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800")
                                }
                            }
                            on:click={
                                let nav_filter = use_navigate();
                                move |_| {
                                    set_filter.set(f_click.clone());
                                    let v = version_param().unwrap_or(1);
                                    nav_filter(
                                        &format!("/definitions/{}/instances?version={}&page=1", def_id(), v),
                                        Default::default(),
                                    );
                                }
                            }
                        >
                            {label}
                        </button>
                    }
                }).collect_view()}
                <input
                    type="text"
                    placeholder="Search by ID or business key…"
                    class="ml-auto w-56 text-xs px-2.5 py-1 rounded border border-gray-200 \
                           dark:border-gray-700 bg-white dark:bg-gray-900 text-gray-900 \
                           dark:text-gray-100 placeholder-gray-400 \
                           focus:outline-none focus:ring-1 focus:ring-indigo-400"
                    on:input=move |ev| set_search.set(event_target_value(&ev))
                />
            </div>

            // ── Activity filter chip ─────────────────────────────────────
            {move || activity_filter.get().map(|eid| view! {
                <div class="flex items-center gap-2 mb-3">
                    <span class="text-xs text-gray-500 dark:text-gray-400">"At:"</span>
                    <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs \
                                 bg-blue-100 dark:bg-blue-950/50 text-blue-700 dark:text-blue-300 \
                                 font-mono">
                        {eid}
                        <button
                            on:click=move |_| set_activity_filter.set(None)
                            class="ml-0.5 text-blue-400 hover:text-blue-700 dark:hover:text-blue-200 \
                                   cursor-pointer leading-none"
                        >"×"</button>
                    </span>
                </div>
            })}

            // ── Instances table ──────────────────────────────────────────
            <InstancesTable
                instances=activity_filtered
                show_definition=false
                show_status_filter=false
                external_search=Signal::derive(move || search.get())
                total_pages=total_pages_sig
                current_page=current_page_sig
                on_page_change=on_page_change
            />
        </div>

        {move || show_start_modal.get().then(|| view! {
            <StartModal
                def_id=Signal::derive(def_id)
                versions=versions_data.into()
                selected_version=Signal::derive(version_param)
                on_close=Callback::new(move |_| set_show_start_modal.set(false))
                on_started=Callback::new(move |_| {
                    instances.refetch();
                    definition.refetch();
                })
            />
        })}
    }
}
