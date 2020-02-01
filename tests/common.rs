use std::path::PathBuf;

pub fn path_to_helper(name: &str) -> PathBuf {
    let mut path = std::env::current_exe().expect("current exe");
    path.pop();
    path.push("examples");
    path.push(name);
    path
}

fn data_file(name: &str) -> PathBuf {
    let mut path = PathBuf::from(file!());
    path.pop();
    path.push("data");
    path.push(name);
    path
}

use decompose::execution::ProgramInfo;

#[derive(PartialEq, Debug)]
pub enum Event {
    Start(),
    Stop(),
    ProgramStarted(ProgramInfo),
    ProgramDied(ProgramInfo),
    ProgramTerminated(ProgramInfo),
    ProgramKilled(ProgramInfo),
}

pub struct ExecutionHandle{
    handle: Option<std::thread::JoinHandle<()>>,
    pub events: std::sync::mpsc::Receiver<Event>,
}

impl ExecutionHandle {
    pub fn new(config: &str) -> ExecutionHandle {
        let path = data_file(config);
        let config = decompose::config::System::from_file(
            path.to_str().expect("path"))
            .expect("config file");
        let (tx, rx) = std::sync::mpsc::channel();
        let listener = EventListener{sender: tx};
        let mut exec = decompose::execution::Execution::from_config(config, listener).expect("start");

        let handle = std::thread::spawn(move || {
            exec.wait();
        });

        let e = rx.recv().expect("recv");
        assert_eq!(Event::Start(), e);

        ExecutionHandle{
            handle: Some(handle),
            events: rx,
        }
    }

    pub fn stop(&mut self) {
        if let Some(h) = self.handle.take() {
            h.join().expect("join");
        }
    }
}

impl Drop for ExecutionHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

struct EventListener {
    sender: std::sync::mpsc::Sender<Event>,
}

impl decompose::execution::Listener for EventListener{
    fn event(&self, e: decompose::execution::Event) {
        let e = match e {
            decompose::execution::Event::Start() => Event::Start(),
            decompose::execution::Event::Stop() => Event::Stop(),
            decompose::execution::Event::ProgramStarted(info) =>
                Event::ProgramStarted(ProgramInfo{name: info.name.clone(), pid: info.pid}),
            decompose::execution::Event::ProgramDied(info) =>
                Event::ProgramStarted(ProgramInfo{name: info.name.clone(), pid: info.pid}),
            decompose::execution::Event::ProgramTerminated(info) =>
                Event::ProgramStarted(ProgramInfo{name: info.name.clone(), pid: info.pid}),
            decompose::execution::Event::ProgramKilled(info) =>
                Event::ProgramStarted(ProgramInfo{name: info.name.clone(), pid: info.pid}),
        };
        self.sender.send(e);
    }
}
