use tokio::task::JoinHandle;
use tokio::sync::mpsc;
use tracing::{error, info};
use std::future::Future;
use std::pin::Pin;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type ActorTask = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

pub struct ActorSupervisor {
    tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    restart_tx: mpsc::Sender<String>,
    restart_rx: Arc<Mutex<mpsc::Receiver<String>>>,
}

impl ActorSupervisor {
    pub fn new() -> Self {
        let (restart_tx, restart_rx) = mpsc::channel(100);
        ActorSupervisor {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            restart_tx,
            restart_rx: Arc::new(Mutex::new(restart_rx)),
        }
    }

    pub async fn supervise(&self) {
        loop {
            let mut rx = self.restart_rx.lock().unwrap();
            if let Some(actor_name) = rx.recv().await {
                info!("Restarting actor task: {}", actor_name);
                // Here you would restart the actor task by spawning it again
                // For demo, just log. Actual restart logic depends on actor creation.
            }
        }
    }

    pub fn add_task(&self, name: String, handle: JoinHandle<()>) {
        let mut tasks = self.tasks.lock().unwrap();
        tasks.insert(name, handle);
    }

    pub fn get_restart_sender(&self) -> mpsc::Sender<String> {
        self.restart_tx.clone()
    }
}
