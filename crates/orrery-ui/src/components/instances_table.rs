use leptos::*;
use leptos_router::*;
use orrery_types::ProcessInstanceResponse;

use crate::components::copy_button::CopyButton;
use crate::components::elapsed_time::ElapsedTime;
use crate::components::empty_state::EmptyState;
use crate::components::skeleton::SkeletonRow;
use crate::components::status_badge::{relative_time, truncate_id, StatusBadge};

/// Generic process instances table with text search and optional status filter tabs.
///
/// The parent controls data fetching; this component owns display, search, and
/// (optionally) client-side status filtering.
///
/// # Props
/// - `instances` — reactive data source; `None` = loading (shows skeleton rows)
/// - `limit` — cap displayed rows after filtering; `0` = unlimited (dashboard uses `20`)
/// - `show_definition` — show "Process" column linking to the definition's instances page
/// - `show_status_filter` — show All/Active/Failed/Completed client-side filter tabs;
///   set to `false` when the parent already provides server-side state filtering
#[component]
pub fn InstancesTable(
    instances: Signal<Option<Vec<ProcessInstanceResponse>>>,
    #[prop(optional, default = 0)] limit: usize,
    #[prop(optional, default = false)] show_definition: bool,
    #[prop(optional, default = true)] show_status_filter: bool,
    /// When provided the parent owns the search input; the table uses this signal
    /// for filtering and does not render its own search box.
    #[prop(optional)]
    external_search: Option<Signal<String>>,
    #[prop(optional)] total_pages: Option<Signal<u32>>,
    #[prop(optional)] current_page: Option<Signal<u32>>,
    #[prop(optional)] on_page_change: Option<Callback<u32>>,
) -> impl IntoView {
    let (internal_search, set_search) = create_signal(String::new());
    let search = external_search.unwrap_or_else(|| internal_search.into());
    let owns_search = external_search.is_none();
    let (status, set_status) = create_signal("all");

    // Column count used for colspan / SkeletonRow. Computed once (props are static).
    let col_count = if show_definition { 6usize } else { 5 };

    let filtered = move || -> Option<Vec<ProcessInstanceResponse>> {
        let list = instances.get()?;

        // 1. Client-side status filter
        let list: Vec<_> = if show_status_filter {
            let f = status.get();
            list.into_iter()
                .filter(|i| match f {
                    "active" => matches!(
                        i.state.as_str(),
                        "RUNNING"
                            | "WAITING_FOR_TASK"
                            | "WAITING_FOR_TIMER"
                            | "WAITING_FOR_MESSAGE"
                            | "WAITING_FOR_SIGNAL"
                    ),
                    "failed" => i.state == "FAILED",
                    "completed" => matches!(i.state.as_str(), "COMPLETED" | "CANCELLED"),
                    _ => true,
                })
                .collect()
        } else {
            list
        };

        // 2. Text search: id prefix OR business_key substring (case-insensitive)
        let q = search.get().to_lowercase();
        let list: Vec<_> = if q.is_empty() {
            list
        } else {
            list.into_iter()
                .filter(|i| {
                    i.id.to_lowercase().starts_with(&q)
                        || i.business_key
                            .as_deref()
                            .unwrap_or("")
                            .to_lowercase()
                            .contains(&q)
                })
                .collect()
        };

        // 3. Row limit
        let list = if limit > 0 && list.len() > limit {
            list.into_iter().take(limit).collect()
        } else {
            list
        };

        Some(list)
    };

    view! {
        <div>
            // ── Toolbar: status tabs + search input ────────────────────────────
            // Only rendered when there's something to show (tabs or own search box).
            {(show_status_filter || owns_search).then(|| view! {
                <div class="flex items-center gap-2 mb-3">
                    {show_status_filter.then(|| view! {
                        <div class="flex gap-1">
                            {["all", "active", "failed", "completed"].into_iter().map(|f| {
                                view! {
                                    <button
                                        class=move || {
                                            let base = "px-2.5 py-0.5 text-xs font-medium rounded-full \
                                                       transition-colors cursor-pointer";
                                            if status.get() == f {
                                                format!("{base} bg-indigo-100 dark:bg-indigo-900 \
                                                        text-indigo-700 dark:text-indigo-300")
                                            } else {
                                                format!("{base} text-gray-500 dark:text-gray-400 \
                                                        hover:bg-gray-100 dark:hover:bg-gray-800")
                                            }
                                        }
                                        on:click=move |_| set_status.set(f)
                                    >
                                        {match f {
                                            "all"       => "All",
                                            "active"    => "Active",
                                            "failed"    => "Failed",
                                            "completed" => "Completed",
                                            _           => f,
                                        }}
                                    </button>
                                }
                            }).collect_view()}
                        </div>
                    })}
                    {owns_search.then(|| view! {
                        <input
                            type="text"
                            placeholder="Search by ID or business key…"
                            class="ml-auto w-56 text-xs px-2.5 py-1 rounded border border-gray-200 \
                                   dark:border-gray-700 bg-white dark:bg-gray-900 text-gray-900 \
                                   dark:text-gray-100 placeholder-gray-400 \
                                   focus:outline-none focus:ring-1 focus:ring-indigo-400"
                            on:input=move |ev| set_search.set(event_target_value(&ev))
                        />
                    })}
                </div>
            })}

            // ── Table ──────────────────────────────────────────────────────────
            <table class="w-full text-sm border-collapse">
                <thead>
                    <tr class="border-b-2 border-gray-200 dark:border-gray-800">
                        <th class="text-left py-2 px-3 text-xs font-medium text-gray-500">"ID"</th>
                        <th class="text-left py-2 px-3 text-xs font-medium text-gray-500">"Business Key"</th>
                        <th class="text-left py-2 px-3 text-xs font-medium text-gray-500">"Status"</th>
                        {show_definition.then(|| view! {
                            <th class="text-left py-2 px-3 text-xs font-medium text-gray-500">"Process"</th>
                        })}
                        <th class="text-left py-2 px-3 text-xs font-medium text-gray-500">"Elapsed"</th>
                        <th class="text-left py-2 px-3 text-xs font-medium text-gray-500">"Age"</th>
                    </tr>
                </thead>
                <tbody>
                    {move || match filtered() {
                        None => view! {
                            <SkeletonRow cols=col_count/>
                            <SkeletonRow cols=col_count/>
                            <SkeletonRow cols=col_count/>
                        }.into_view(),
                        Some(list) if list.is_empty() => view! {
                            <tr>
                                <td colspan=col_count class="py-0">
                                    <EmptyState title="No instances"/>
                                </td>
                            </tr>
                        }.into_view(),
                        Some(list) => list.into_iter().map(|inst| {
                            let id       = inst.id.clone();
                            let bk       = inst.business_key.as_deref().unwrap_or("—").to_string();
                            let age      = relative_time(&inst.created_at);
                            let is_failed    = inst.state == "FAILED";
                            let is_cancelled = inst.state == "CANCELLED";
                            let def_id   = inst.process_definition_id.clone();
                            let def_href = format!("/definitions/{}/instances", inst.process_definition_id);
                            let nav      = format!("/instances/{}", inst.id);
                            view! {
                                <tr
                                    class=move || {
                                        let base = "border-b border-gray-100 dark:border-gray-900 \
                                                   hover:bg-gray-50 dark:hover:bg-gray-900/50 \
                                                   cursor-pointer";
                                        if is_failed {
                                            format!("{base} border-l-2 border-l-red-500")
                                        } else if is_cancelled {
                                            format!("{base} border-l-2 border-l-gray-400")
                                        } else {
                                            base.to_string()
                                        }
                                    }
                                    on:click={
                                        let nav = nav.clone();
                                        move |_| { use_navigate()(&nav, Default::default()); }
                                    }
                                >
                                    <td class="py-2.5 px-3 font-mono text-xs text-gray-700 dark:text-gray-300">
                                        {id}
                                        <CopyButton text=inst.id.clone()/>
                                    </td>
                                    <td class="py-2.5 px-3 text-xs text-gray-600 dark:text-gray-400">
                                        {bk}
                                    </td>
                                    <td class="py-2.5 px-3">
                                        <StatusBadge state=inst.state.clone()/>
                                    </td>
                                    {show_definition.then(|| {
                                        let d = truncate_id(&def_id);
                                        view! {
                                            <td class="py-2.5 px-3 font-mono text-xs text-gray-500 \
                                                       max-w-[120px] truncate"
                                                title=def_id.clone()>
                                                <A
                                                    href=def_href.clone()
                                                    class="hover:text-indigo-600 dark:hover:text-indigo-400"
                                                    on:click=|ev| ev.stop_propagation()
                                                >
                                                    {d}
                                                </A>
                                            </td>
                                        }
                                    })}
                                    <td class="py-2.5 px-3 text-xs text-gray-500">
                                        <ElapsedTime created_at=inst.created_at ended_at=inst.ended_at/>
                                    </td>
                                    <td class="py-2.5 px-3 text-xs text-gray-500">{age}</td>
                                </tr>
                            }
                        }).collect_view().into_view(),
                    }}
                </tbody>
            </table>

            // ── Pagination bar (only when all three props are provided) ────────────
            {match (total_pages, current_page, on_page_change) {
                (Some(total_p), Some(cur_p), Some(on_change)) => {
                    view! {
                        <div class="flex items-center justify-center gap-1 mt-4 select-none">
                            // Prev button
                            <button
                                disabled=move || cur_p.get() <= 1
                                on:click=move |_| {
                                    let p = cur_p.get();
                                    if p > 1 { on_change.call(p - 1); }
                                }
                                class="px-2.5 py-1 text-xs rounded border border-gray-200 dark:border-gray-700 \
                                       text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800 \
                                       disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer \
                                       transition-colors"
                            >
                                "← Prev"
                            </button>

                            // Page number buttons
                            {move || {
                                let total = total_p.get();
                                let cur = cur_p.get();
                                let slots = build_page_slots(cur, total);
                                slots.into_iter().map(|slot| match slot {
                                    Some(n) => {
                                        let is_cur = n == cur;
                                        view! {
                                            <button
                                                on:click=move |_| on_change.call(n)
                                                class=move || {
                                                    let base = "px-2.5 py-1 text-xs rounded border cursor-pointer \
                                                               transition-colors";
                                                    if is_cur {
                                                        format!("{base} bg-indigo-100 dark:bg-indigo-900 \
                                                                border-indigo-300 dark:border-indigo-700 \
                                                                text-indigo-700 dark:text-indigo-300 font-medium")
                                                    } else {
                                                        format!("{base} border-gray-200 dark:border-gray-700 \
                                                                text-gray-600 dark:text-gray-400 \
                                                                hover:bg-gray-100 dark:hover:bg-gray-800")
                                                    }
                                                }
                                            >
                                                {n.to_string()}
                                            </button>
                                        }.into_view()
                                    }
                                    None => view! {
                                        <span class="px-1 py-1 text-xs text-gray-400">"…"</span>
                                    }.into_view(),
                                }).collect_view()
                            }}

                            // Next button
                            <button
                                disabled=move || total_p.get() <= cur_p.get()
                                on:click=move |_| {
                                    let p = cur_p.get();
                                    let t = total_p.get();
                                    if p < t { on_change.call(p + 1); }
                                }
                                class="px-2.5 py-1 text-xs rounded border border-gray-200 dark:border-gray-700 \
                                       text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800 \
                                       disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer \
                                       transition-colors"
                            >
                                "Next →"
                            </button>
                        </div>
                    }.into_view()
                }
                _ => ().into_view(),
            }}
        </div>
    }
}

/// Returns a list of page numbers and None (ellipsis) for the pagination bar.
/// Shows up to 7 slots: always includes first, last, current, and ±2 around current.
fn build_page_slots(current: u32, total: u32) -> Vec<Option<u32>> {
    if total <= 7 {
        return (1..=total).map(Some).collect();
    }
    let mut shown: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    shown.insert(1);
    shown.insert(total);
    for d in 0..=2u32 {
        if current > d {
            shown.insert(current - d);
        }
        if current + d <= total {
            shown.insert(current + d);
        }
    }
    let mut slots = Vec::new();
    let mut prev: Option<u32> = None;
    for n in shown {
        if let Some(p) = prev {
            if n > p + 1 {
                slots.push(None); // ellipsis
            }
        }
        slots.push(Some(n));
        prev = Some(n);
    }
    slots
}
