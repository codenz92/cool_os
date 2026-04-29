extern crate alloc;

use alloc::{format, string::String, vec::Vec};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Term,
    Int,
    User1,
}

impl Signal {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "term" | "TERM" | "sigterm" => Some(Signal::Term),
            "int" | "INT" | "sigint" => Some(Signal::Int),
            "usr1" | "USR1" | "user1" => Some(Signal::User1),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Signal::Term => "TERM",
            Signal::Int => "INT",
            Signal::User1 => "USR1",
        }
    }
}

pub fn status_lines() -> Vec<String> {
    let sched = crate::scheduler::SCHEDULER.lock();
    let mut lines = Vec::new();
    for (pid, task) in sched.tasks.iter().enumerate() {
        let mut parent = String::new();
        if let Some(id) = task.parent {
            push_usize(&mut parent, id);
        } else {
            parent.push('-');
        }
        lines.push(format!(
            "pid={} ppid={} pgid={} signal={} status={:?} name={}",
            pid,
            parent,
            task.process_group,
            task.pending_signal
                .map(|signal| signal.label())
                .unwrap_or("-"),
            task.status,
            task.name
        ));
    }
    lines
}

pub fn zombie_policy_lines() -> Vec<String> {
    alloc::vec![
        String::from("policy: exited children remain zombies until waitpid/reap"),
        String::from("shell reap command may reap all exited tasks"),
        String::from("future: service supervisor will reap owned service children"),
    ]
}

fn push_usize(out: &mut String, mut value: usize) {
    if value == 0 {
        out.push('0');
        return;
    }
    let mut digits = [0u8; 20];
    let mut len = 0usize;
    while value > 0 {
        digits[len] = b'0' + (value % 10) as u8;
        value /= 10;
        len += 1;
    }
    for idx in (0..len).rev() {
        out.push(digits[idx] as char);
    }
}
