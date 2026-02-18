mod activity_tab;
use activity_tab::ActivityTab;
mod jobs_tab;
use jobs_tab::JobsTab;
mod task_banner;
use task_banner::TaskBanner;
mod variables_tab;
use variables_tab::VariablesTab;

use leptos::*;
use leptos_router::*;

use crate::api;
use crate::components::bpmn_viewer::BpmnViewer;
use crate::components::copy_button::CopyButton;
use crate::components::page_breadcrumb::PageBreadcrumb;
use crate::components::status_badge::StatusBadge;
use orrery_types::{HistoryEntryResponse, ProcessInstanceResponse, TaskResponse, TimerResponse};

#[component]
pub fn InstancePage() -> impl IntoView {
    let params = use_params_map();
    let id = move || params.with(|p| p.get("id").cloned().unwrap_or_default());

    let (history_level, set_history_level) = create_signal("activity".to_string());

    let instance = create_resource(id, |id| async move { api::get_instance(&id).await });
    let history = create_resource(
        move || (id(), history_level.get()),
        |(id, level)| async move { api::get_instance_history(&id, Some(&level)).await },
    );
    let tasks = create_resource(
        id,
        |id| async move { api::list_tasks_for_instance(&id).await },
    );
    let timers = create_resource(id, |id| async move { api::get_instance_timers(&id).await });

    // Anti-flicker: retain last successful data in signals so the UI doesn't
    // flash a loading state on every 3s poll refetch.
    let (instance_data, set_instance_data) = create_signal(Option::<ProcessInstanceResponse>::None);
    let (history_data, set_history_data) = create_signal(Option::<Vec<HistoryEntryResponse>>::None);
    let (tasks_data, set_tasks_data) = create_signal(Option::<Vec<TaskResponse>>::None);
    let (timers_data, set_timers_data) = create_signal(Option::<Vec<TimerResponse>>::None);

    create_effect(move |_| {
        if let Some(Ok(v)) = instance.get() {
            set_instance_data.set(Some(v));
        }
    });
    create_effect(move |_| {
        if let Some(Ok(v)) = history.get() {
            set_history_data.set(Some(v));
        }
    });
    create_effect(move |_| {
        if let Some(Ok(v)) = tasks.get() {
            set_tasks_data.set(Some(v));
        }
    });
    create_effect(move |_| {
        if let Some(Ok(v)) = timers.get() {
            set_timers_data.set(Some(v));
        }
    });

    let (action_status, set_action_status) = create_signal(Option::<(bool, String)>::None);
    let (acting, set_acting) = create_signal(false);
    let (active_tab, set_active_tab) = create_signal("variables");
    let (show_error, set_show_error) = create_signal(false);
    let (details_expanded, set_details_expanded) = create_signal(false);

    // Auto-refresh every 3s while instance is in an active state.
    {
        let instance_ref = instance.clone();
        let history_ref = history.clone();
        let tasks_ref = tasks.clone();
        let timers_ref = timers.clone();
        spawn_local(async move {
            loop {
                gloo_timers::future::sleep(std::time::Duration::from_millis(3000)).await;
                if let Some(Ok(ref inst)) = instance_ref.get() {
                    let active = matches!(
                        inst.state.as_str(),
                        "RUNNING"
                            | "WAITING_FOR_TASK"
                            | "WAITING_FOR_TIMER"
                            | "WAITING_FOR_MESSAGE"
                            | "WAITING_FOR_SIGNAL"
                    );
                    if active {
                        instance_ref.refetch();
                        history_ref.refetch();
                        tasks_ref.refetch();
                        timers_ref.refetch();
                    }
                }
            }
        });
    }

    let do_cancel = move |inst_id: String| {
        set_acting.set(true);
        set_action_status.set(None);
        spawn_local(async move {
            match api::cancel_instance(&inst_id).await {
                Ok(_) => {
                    set_action_status.set(Some((true, "Instance cancelled.".to_string())));
                    instance.refetch();
                    history.refetch();
                    tasks.refetch();
                }
                Err(e) => set_action_status.set(Some((false, e))),
            }
            set_acting.set(false);
        });
    };

    let do_retry_task = move |task_id: String| {
        set_acting.set(true);
        set_action_status.set(None);
        spawn_local(async move {
            match api::retry_task(&task_id).await {
                Ok(_) => {
                    set_action_status.set(Some((true, "Task queued for retry.".to_string())));
                    instance.refetch();
                    tasks.refetch();
                }
                Err(e) => set_action_status.set(Some((false, e))),
            }
            set_acting.set(false);
        });
    };

    let do_retry_instance = move |inst_id: String| {
        set_acting.set(true);
        set_action_status.set(None);
        spawn_local(async move {
            match api::retry_instance(&inst_id).await {
                Ok(_) => {
                    set_action_status.set(Some((true, "Instance retried.".to_string())));
                    instance.refetch();
                    history.refetch();
                    tasks.refetch();
                    timers.refetch();
                }
                Err(e) => set_action_status.set(Some((false, e))),
            }
            set_acting.set(false);
        });
    };

    let do_fast_forward = {
        let instance = instance.clone();
        let timers = timers.clone();
        move |timer_id: String| {
            let inst_id = id();
            set_acting.set(true);
            set_action_status.set(None);
            let instance = instance.clone();
            let timers = timers.clone();
            spawn_local(async move {
                match api::fast_forward_timer(&inst_id, &timer_id).await {
                    Ok(_) => {
                        set_action_status.set(Some((true, "Timer fired.".to_string())));
                        instance.refetch();
                        timers.refetch();
                    }
                    Err(e) => set_action_status.set(Some((false, e))),
                }
                set_acting.set(false);
            });
        }
    };

    let do_update_timer = {
        let timers = timers.clone();
        move |(timer_id, expression): (String, String)| {
            let inst_id = id();
            set_acting.set(true);
            set_action_status.set(None);
            let timers = timers.clone();
            spawn_local(async move {
                match api::update_timer_expression(&inst_id, &timer_id, expression).await {
                    Ok(_) => {
                        set_action_status.set(Some((true, "Timer rescheduled.".to_string())));
                        timers.refetch();
                    }
                    Err(e) => set_action_status.set(Some((false, e))),
                }
                set_acting.set(false);
            });
        }
    };

    view! {
        <div class="p-6">

            // ── Breadcrumb ──────────────────────────────────────────────────
            <PageBreadcrumb>
                {move || match instance_data.get() {
                    None => view! {
                        <span class="font-mono text-xs text-gray-400">{id()}</span>
                    }.into_view(),
                    Some(inst) => {
                        let def_href = format!(
                            "/definitions/{}/instances?version={}&page=1",
                            inst.process_definition_id,
                            inst.process_definition_version,
                        );
                        let def_label = format!(
                            "{}:{}",
                            inst.process_definition_id,
                            inst.process_definition_version,
                        );
                        view! {
                            <A
                                href=def_href
                                class="text-sm font-semibold text-indigo-600 dark:text-indigo-400 hover:underline"
                            >
                                {def_label}
                            </A>
                            <span class="text-gray-300 dark:text-gray-600">"|"</span>
                            <span class="font-mono text-xs text-gray-700 dark:text-gray-300">
                                {inst.id.clone()}
                            </span>
                            <CopyButton text=inst.id.clone()/>
                        }.into_view()
                    }
                }}
            </PageBreadcrumb>

            // ── Collapsible instance details ────────────────────────────────
            {move || instance_data.get().map(|inst| {
                let is_active = matches!(
                    inst.state.as_str(),
                    "RUNNING" | "WAITING_FOR_TASK" | "WAITING_FOR_TIMER" | "WAITING_FOR_MESSAGE" | "WAITING_FOR_SIGNAL"
                );
                let inst_id = inst.id.clone();
                view! {
                    <div class="mb-4">
                        // Collapsed row: chevron + status badge (always visible)
                        <button
                            on:click=move |_| set_details_expanded.update(|v| *v = !*v)
                            class="flex items-center gap-2 px-2 py-1 rounded \
                                   hover:bg-gray-50 dark:hover:bg-gray-800/50 \
                                   transition-colors cursor-pointer w-full"
                        >
                            <svg
                                xmlns="http://www.w3.org/2000/svg"
                                width="12" height="12"
                                viewBox="0 0 24 24"
                                fill="none" stroke="currentColor"
                                stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
                                class=move || format!(
                                    "text-gray-400 transition-transform duration-200 {}",
                                    if details_expanded.get() { "rotate-90" } else { "" }
                                )
                            >
                                <polyline points="9 18 15 12 9 6"/>
                            </svg>
                            <StatusBadge state=inst.state.clone()/>
                        </button>

                        // Expanded panel
                        {move || details_expanded.get().then(|| {
                            let inst_id = inst_id.clone();
                            view! {
                                <div class="mt-1 ml-6 px-3 py-2.5 rounded-lg border \
                                            border-gray-200 dark:border-gray-700 \
                                            bg-gray-50 dark:bg-gray-800/40 \
                                            flex flex-wrap items-center gap-x-4 gap-y-2 text-xs">
                                    {inst.business_key.as_ref().map(|bk| view! {
                                        <span class="text-gray-500 dark:text-gray-400">
                                            "Key: "
                                            <span class="font-mono text-gray-700 dark:text-gray-300">
                                                {bk.clone()}
                                            </span>
                                        </span>
                                    })}
                                    <crate::components::elapsed_time::ElapsedTime
                                        created_at=inst.created_at
                                        ended_at=inst.ended_at
                                    />
                                    {move || action_status.get().map(|(ok, msg)| view! {
                                        <span class=if ok {
                                            "text-emerald-600 dark:text-emerald-400"
                                        } else {
                                            "text-red-500"
                                        }>{msg}</span>
                                    })}
                                    {is_active.then(|| {
                                        let inst_id = inst_id.clone();
                                        view! {
                                            <button
                                                disabled=move || acting.get()
                                                on:click=move |_| do_cancel(inst_id.clone())
                                                class="ml-auto px-2.5 py-1 text-xs font-medium rounded \
                                                       border border-red-300 text-red-600 dark:text-red-400 \
                                                       hover:bg-red-50 dark:hover:bg-red-950/30 \
                                                       disabled:opacity-50 transition-colors cursor-pointer"
                                            >
                                                "Cancel"
                                            </button>
                                        }
                                    })}
                                </div>
                            }
                        })}
                    </div>
                }
            })}

            // ── Active/failed task banner ─────────────────────────────────────
            <TaskBanner
                instance_data=instance_data
                tasks_data=tasks_data
                acting=acting
                on_retry=Callback::new(move |task_id| do_retry_task(task_id))
                open_error=Callback::new(move |_| set_show_error.set(true))
            />

            // ── Instance error banner (engine-level failures) ────────────────
            {move || {
                let inst = instance_data.get()?;
                if inst.state != "FAILED" { return None; }
                let error_msg = inst.error_message.clone()?;
                let inst_id = inst.id.clone();
                let truncated = if error_msg.len() > 120 {
                    format!("{}...", &error_msg[..120])
                } else {
                    error_msg
                };
                Some(view! {
                    <div class="mb-4 flex items-center gap-3 px-4 py-2.5 rounded-lg border \
                                border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-950/30">
                        <span class="text-red-600 dark:text-red-400 text-xs font-medium uppercase tracking-wider shrink-0">
                            "Failed"
                        </span>
                        <code class="text-xs font-mono text-gray-700 dark:text-gray-300 flex-1 truncate">
                            {truncated}
                        </code>
                        <button
                            disabled=move || acting.get()
                            on:click=move |_| do_retry_instance(inst_id.clone())
                            class="px-2 py-0.5 text-xs rounded border border-amber-400 \
                                   text-amber-700 dark:text-amber-400 hover:bg-amber-50 \
                                   dark:hover:bg-amber-950/30 disabled:opacity-50 shrink-0 cursor-pointer"
                        >"Retry"</button>
                        <button
                            on:click=move |_| set_show_error.set(true)
                            class="px-2 py-0.5 text-xs rounded border border-red-300 \
                                   text-red-600 dark:text-red-400 hover:bg-red-50 \
                                   dark:hover:bg-red-950/30 shrink-0 cursor-pointer"
                        >"Error"</button>
                    </div>
                })
            }}

            // ── Error modal ───────────────────────────────────────────────────
            {move || show_error.get().then(|| {
                let msg = instance_data.get()
                    .and_then(|i| i.error_message)
                    .unwrap_or_else(|| "No error message recorded.".to_string());
                let msg2 = msg.clone();
                view! {
                    <div
                        class="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
                        on:click=move |ev| {
                            if ev.target() == ev.current_target() { set_show_error.set(false); }
                        }
                    >
                        <div class="bg-white dark:bg-gray-900 rounded-xl shadow-xl w-full max-w-lg mx-4 p-6">
                            <div class="flex items-center justify-between mb-3">
                                <h2 class="text-sm font-semibold text-red-700 dark:text-red-400">
                                    "Instance Error"
                                </h2>
                                <button
                                    on:click=move |_| set_show_error.set(false)
                                    class="text-lg text-gray-400 hover:text-gray-600 \
                                           dark:hover:text-gray-300 cursor-pointer leading-none"
                                >"×"</button>
                            </div>
                            <div class="flex items-start gap-2 bg-red-50 dark:bg-red-950/30 rounded-lg p-3">
                                <code class="text-xs font-mono text-red-800 dark:text-red-300 flex-1 \
                                             break-all whitespace-pre-wrap">
                                    {msg}
                                </code>
                                <CopyButton text=msg2/>
                            </div>
                        </div>
                    </div>
                }
            })}

            // ── Diagram (full width) ──────────────────────────────────────────
            <div class="mb-6">
                <BpmnViewer diagram_url=move || {
                    // Include active_element_ids and state as a cache-busting param so
                    // the resource refetches when the diagram content changes (task advances,
                    // instance fails, etc.). The server ignores the query param.
                    let version = instance_data.get()
                        .map(|i| format!("{}-{}", i.state, i.active_element_ids))
                        .unwrap_or_default();
                    format!("/v1/process-instances/{}/diagram?_={}", id(), version)
                } />
            </div>

            // ── Tabs: Variables | Activity | Jobs ─────────────────────────────
            <div class="border border-gray-200 dark:border-gray-800 rounded-lg overflow-hidden">

                // Tab bar
                <div class="flex border-b border-gray-200 dark:border-gray-800 bg-gray-50 dark:bg-gray-900">
                    <button
                        on:click=move |_| set_active_tab.set("variables")
                        class=move || {
                            let base = "px-4 py-2.5 text-xs font-medium uppercase tracking-wider \
                                        border-b-2 -mb-px transition-colors cursor-pointer";
                            if active_tab.get() == "variables" {
                                format!("{base} border-indigo-500 text-indigo-600 dark:text-indigo-400")
                            } else {
                                format!("{base} border-transparent text-gray-500 dark:text-gray-400 \
                                         hover:text-gray-700 dark:hover:text-gray-300")
                            }
                        }
                    >
                        "Variables"
                    </button>
                    <button
                        on:click=move |_| set_active_tab.set("activity")
                        class=move || {
                            let base = "px-4 py-2.5 text-xs font-medium uppercase tracking-wider \
                                        border-b-2 -mb-px transition-colors cursor-pointer";
                            if active_tab.get() == "activity" {
                                format!("{base} border-indigo-500 text-indigo-600 dark:text-indigo-400")
                            } else {
                                format!("{base} border-transparent text-gray-500 dark:text-gray-400 \
                                         hover:text-gray-700 dark:hover:text-gray-300")
                            }
                        }
                    >
                        "Activity"
                    </button>
                    <button
                        on:click=move |_| set_active_tab.set("jobs")
                        class=move || {
                            let base = "px-4 py-2.5 text-xs font-medium uppercase tracking-wider \
                                        border-b-2 -mb-px transition-colors cursor-pointer";
                            if active_tab.get() == "jobs" {
                                format!("{base} border-indigo-500 text-indigo-600 dark:text-indigo-400")
                            } else {
                                format!("{base} border-transparent text-gray-500 dark:text-gray-400 \
                                         hover:text-gray-700 dark:hover:text-gray-300")
                            }
                        }
                    >
                        "Jobs"
                    </button>
                </div>

                // Variables tab content
                {move || (active_tab.get() == "variables").then(|| {
                    view! {
                        <VariablesTab
                            instance_data=instance_data
                            on_save=Callback::new(move |_| { instance.refetch(); })
                            set_action_status=set_action_status
                        />
                    }.into_view()
                })}

                // Activity tab content
                {move || (active_tab.get() == "activity").then(|| {
                    view! { <ActivityTab history_data=history_data level=history_level set_level=set_history_level/> }.into_view()
                })}

                // Jobs tab content
                {move || (active_tab.get() == "jobs").then(|| {
                    view! {
                        <JobsTab
                            timers_data=timers_data
                            acting=acting
                            on_fast_forward=Callback::new(do_fast_forward.clone())
                            on_update=Callback::new(do_update_timer.clone())
                        />
                    }.into_view()
                })}

            </div>
        </div>
    }
}
