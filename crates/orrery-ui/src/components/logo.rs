use leptos::*;

#[component]
pub fn OrreryIcon() -> impl IntoView {
    view! {
        <img class="block dark:hidden w-6 h-6 shrink-0" src="/orrery-icon-transparent.svg" alt="Orrery"/>
        <img class="hidden dark:block w-6 h-6 shrink-0" src="/orrery-icon-dark.svg" alt="Orrery"/>
    }
}
