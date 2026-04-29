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
    pub restarts: u32,
    pub last_tick: u64,
    pub loops: u64,
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
    crate::profiler::record_service("supervisor", "initialized");
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

pub fn supervise() {
    let now = crate::interrupts::ticks();
    let mut services = SERVICES.lock();
    for service in services.iter_mut() {
        if service.state == ServiceState::Failed
            && (service.restart == "always" || service.restart == "on-failure")
        {
            service.state = ServiceState::Running;
            service.restarts = service.restarts.saturating_add(1);
            service.last_tick = now;
            crate::event_bus::emit("services", "restart", service.name);
            crate::profiler::record_service(service.name, "restarted");
        }

        if service.state != ServiceState::Running {
            continue;
        }
        let interval = match service.name {
            "search-index" => crate::interrupts::ticks_for_millis(5000),
            "package-db" => crate::interrupts::ticks_for_millis(3000),
            "notification-center" | "event-bus" => crate::interrupts::ticks_for_millis(1000),
            _ => crate::interrupts::ticks_for_millis(1500),
        };
        if service.last_tick == 0 || now.wrapping_sub(service.last_tick) >= interval {
            service.last_tick = now;
            service.loops = service.loops.saturating_add(1);
            match service.name {
                "search-index" => {
                    crate::deferred::enqueue(crate::deferred::DeferredWork::RefreshSearchIndex)
                }
                "package-db" => {
                    crate::deferred::enqueue(crate::deferred::DeferredWork::FlushFilesystemJournal)
                }
                "event-bus" | "notification-center" => {
                    crate::deferred::enqueue(crate::deferred::DeferredWork::FlushKernelLog)
                }
                _ => {}
            }
        }
    }
}

pub fn lines() -> Vec<String> {
    SERVICES
        .lock()
        .iter()
        .map(|service| {
            format!(
                "{:02} {} state={} restart={} restarts={} loops={} last_tick={}",
                service.order,
                service.name,
                service.state.label(),
                service.restart,
                service.restarts,
                service.loops,
                service.last_tick
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
    crate::profiler::record_service(service.name, state.label());
    true
}

fn service(name: &'static str, order: u8, restart: &'static str) -> Service {
    Service {
        name,
        order,
        restart,
        state: ServiceState::Running,
        restarts: 0,
        last_tick: 0,
        loops: 0,
    }
}
