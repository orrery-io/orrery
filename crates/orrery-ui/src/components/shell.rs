use leptos::*;
use leptos_router::*;

use super::logo::OrreryIcon;

#[component]
pub fn Shell(children: Children) -> impl IntoView {
    view! {
        <div class="flex h-screen overflow-hidden bg-white dark:bg-gray-950">
            // Sidebar
            <aside class="w-[220px] shrink-0 flex flex-col bg-gray-50 dark:bg-gray-900 border-r border-gray-200 dark:border-gray-800">
                // Logo
                <div class="px-4 py-4 flex items-center gap-2 border-b border-gray-200 dark:border-gray-800">
                    <OrreryIcon/>
                    <span class="text-sm font-semibold tracking-tight text-gray-900 dark:text-gray-100">"Orrery"</span>
                </div>

                // Nav
                <nav class="flex-1 px-2 py-4 flex flex-col gap-1">
                    <p class="px-2 mb-1 text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">
                        "Engine"
                    </p>
                    <NavItem href="/dashboard" icon="◈" label="Dashboard"/>
                    <NavItem href="/definitions" icon="⬡" label="Processes"/>
                    <NavItem href="/incidents" icon="⚠" label="Incidents"/>
                </nav>

                // Bottom
                <div class="px-2 py-4 border-t border-gray-200 dark:border-gray-800">
                    <NavItem href="/settings" icon="⚙" label="Settings"/>
                </div>
            </aside>

            // Main content
            <main class="flex-1 overflow-auto">
                {children()}
            </main>
        </div>
    }
}

#[component]
fn NavItem(href: &'static str, icon: &'static str, label: &'static str) -> impl IntoView {
    view! {
        <A
            href=href
            class="flex items-center gap-2 px-2 py-1.5 rounded text-sm text-gray-700 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-gray-800 transition-colors"
            active_class="bg-indigo-50 dark:bg-indigo-950 text-indigo-700 dark:text-indigo-300 font-medium"
        >
            <span class="w-4 text-center text-xs">{icon}</span>
            <span>{label}</span>
        </A>
    }
}
