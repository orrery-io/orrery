mod api;
mod app;
mod components;
mod pages;

use app::App;
use leptos::*;

fn main() {
    mount_to_body(|| view! { <App/> })
}
