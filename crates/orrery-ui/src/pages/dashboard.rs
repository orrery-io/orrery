use leptos::*;
use leptos_router::*;

use crate::api;
use crate::components::instances_table::InstancesTable;

#[component]
pub fn DashboardPage() -> impl IntoView {
    let metrics = create_resource(|| (), |_| async { api::get_overview_metrics().await });
    let instances = create_resource(|| (), |_| async { api::list_instances(None, None).await });

    // Anti-flicker: retain last successful data so the UI doesn't flash on refetch
    let (metrics_data, set_metrics_data) =
        create_signal(Option::<orrery_types::OverviewMetrics>::None);
    let (instances_data, set_instances_data) =
        create_signal(Option::<Vec<orrery_types::ProcessInstanceResponse>>::None);

    create_effect(move |_| {
        if let Some(Ok(m)) = metrics.get() {
            set_metrics_data.set(Some(m));
        }
    });
    create_effect(move |_| {
        if let Some(Ok(paginated)) = instances.get() {
            set_instances_data.set(Some(paginated.items));
        }
    });

    // Poll every 10s
    spawn_local(async move {
        loop {
            gloo_timers::future::sleep(std::time::Duration::from_secs(10)).await;
            metrics.refetch();
            instances.refetch();
        }
    });

    view! {
        <div class="p-6">
            // ── Summary bar ──────────────────────────────────────────────────
            {move || {
                let m = metrics_data.get();
                view! {
                    <div class="flex items-center gap-3 mb-6 flex-wrap">
                        <h1 class="text-xl font-semibold text-gray-900 dark:text-gray-100 mr-2">"Dashboard"</h1>
                        <StatCard
                            label="Running"
                            count=m.as_ref().map(|m| m.running_instances).unwrap_or(0)
                            color="blue"
                            href="/definitions"
                        />
                        <StatCard
                            label="Waiting"
                            count=m.as_ref().map(|m| m.waiting_instances).unwrap_or(0)
                            color="amber"
                            href="/definitions"
                        />
                        <StatCard
                            label="Failed"
                            count=m.as_ref().map(|m| m.failed_instances).unwrap_or(0)
                            color="red"
                            href="/incidents"
                        />
                        <StatCard
                            label="Completed"
                            count=m.as_ref().map(|m| m.completed_instances).unwrap_or(0)
                            color="emerald"
                            href="/definitions"
                        />
                    </div>
                }
            }}

            // ── Recent Instances ─────────────────────────────────────────────
            <h2 class="text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400 mb-3">
                "Recent Instances"
            </h2>
            <InstancesTable
                instances=Signal::derive(move || instances_data.get())
                limit=20
                show_definition=true
                show_status_filter=true
            />
        </div>
    }
}

#[component]
fn StatCard(
    label: &'static str,
    count: i64,
    color: &'static str,
    href: &'static str,
) -> impl IntoView {
    let (bg, text, border, dot) = match color {
        "blue" => (
            "bg-blue-50 dark:bg-blue-950/30",
            "text-blue-700 dark:text-blue-300",
            "border-blue-200 dark:border-blue-800",
            "bg-blue-500",
        ),
        "amber" => (
            "bg-amber-50 dark:bg-amber-950/30",
            "text-amber-700 dark:text-amber-300",
            "border-amber-200 dark:border-amber-800",
            "bg-amber-500",
        ),
        "red" => (
            "bg-red-50 dark:bg-red-950/30",
            "text-red-700 dark:text-red-300",
            "border-red-200 dark:border-red-800",
            "bg-red-500",
        ),
        "emerald" => (
            "bg-emerald-50 dark:bg-emerald-950/30",
            "text-emerald-700 dark:text-emerald-300",
            "border-emerald-200 dark:border-emerald-800",
            "bg-emerald-500",
        ),
        "violet" => (
            "bg-violet-50 dark:bg-violet-950/30",
            "text-violet-700 dark:text-violet-300",
            "border-violet-200 dark:border-violet-800",
            "bg-violet-500",
        ),
        _ => (
            "bg-gray-50 dark:bg-gray-900",
            "text-gray-700 dark:text-gray-300",
            "border-gray-200 dark:border-gray-800",
            "bg-gray-500",
        ),
    };
    view! {
        <A
            href=href
            class=format!("flex items-center gap-1.5 px-3 py-1 rounded-full text-sm font-medium \
                           border {bg} {text} {border} cursor-pointer \
                           hover:opacity-80 transition-opacity")
        >
            <span class=format!("w-2 h-2 rounded-full inline-block {dot}")></span>
            {count}" "{label}
        </A>
    }
}
