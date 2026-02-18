use leptos::*;
use leptos_router::*;

use crate::components::shell::Shell;
use crate::pages::{
    dashboard::DashboardPage, definition_instances::DefinitionInstancesPage,
    definitions::DefinitionsPage, incidents::IncidentsPage, instance::InstancePage,
};

#[component]
pub fn App() -> impl IntoView {
    view! {
        <Router>
            <Shell>
                <Routes>
                    <Route path="/" view=|| view! { <Redirect path="/dashboard"/> }/>
                    <Route path="/dashboard" view=DashboardPage/>
                    <Route path="/definitions" view=DefinitionsPage/>
                    <Route path="/definitions/:id/instances" view=DefinitionInstancesPage/>
                    <Route path="/instances/:id" view=InstancePage/>
                    <Route path="/incidents" view=IncidentsPage/>
                    <Route path="/settings" view=|| view! {
                        <div class="p-6">
                            <h1 class="text-xl font-semibold text-gray-900 dark:text-gray-100">"Settings"</h1>
                            <p class="mt-2 text-sm text-gray-500 dark:text-gray-400">"Settings coming in a future phase."</p>
                        </div>
                    }/>
                </Routes>
            </Shell>
        </Router>
    }
}
