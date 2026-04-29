extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use spin::Mutex;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    Stopped,
    Running,
    Failed,
}

impl ServiceState {
    pub const fn label(self) -> &'static str {
        match self {
            ServiceState::Stopped => "stopped",
            ServiceState::Running => "running",
            ServiceState::Failed => "failed",
        }
    }
}

#[derive(Clone)]
pub struct Service {
    pub name: &'static str,
    pub order: u8,
    pub restart: &'static str,
    pub state: ServiceState,
}

static SERVICES: Mutex<Vec<Service>> = Mutex::new(Vec::new());

pub fn init() {
    let mut services = alloc::vec![
        service("event-bus", 1, "always"),
        service("device-registry", 2, "always"),
        service("search-index", 3, "manual"),
        service("package-db", 4, "on-failure"),
        service("notification-center", 5, "always"),
        service("network-stack", 6, "manual"),
        service("power-manager", 7, "manual"),
    ];
    services.sort_by(|a, b| a.order.cmp(&b.order));
    *SERVICES.lock() = services;
    crate::event_bus::emit("services", "boot", "service supervisor initialized");
}

pub fn start(name: &str) -> bool {
    set_state(name, ServiceState::Running)
}

pub fn stop(name: &str) -> bool {
    set_state(name, ServiceState::Stopped)
}

pub fn fail(name: &str) -> bool {
    set_state(name, ServiceState::Failed)
}

pub fn lines() -> Vec<String> {
    SERVICES
        .lock()
        .iter()
        .map(|service| {
            format!(
                "{:02} {} state={} restart={}",
                service.order,
                service.name,
                service.state.label(),
                service.restart
            )
        })
        .collect()
}

fn set_state(name: &str, state: ServiceState) -> bool {
    let mut services = SERVICES.lock();
    let Some(service) = services
        .iter_mut()
        .find(|service| service.name.eq_ignore_ascii_case(name))
    else {
        return false;
    };
    service.state = state;
    crate::event_bus::emit("services", state.label(), service.name);
    true
}

fn service(name: &'static str, order: u8, restart: &'static str) -> Service {
    Service {
        name,
        order,
        restart,
        state: ServiceState::Running,
    }
}
