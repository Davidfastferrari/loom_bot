use eyre::Result;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use tracing::{error, info};

use crate::{Actor, WorkerResult};

#[derive(Default)]
pub struct ActorsManager {
    tasks: Vec<JoinHandle<WorkerResult>>,
}

impl ActorsManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(&mut self, actor: impl Actor + 'static) -> Result<()> {
        match actor.start() {
            Ok(workers) => {
                info!("{} started successfully", actor.name());
                for worker in workers {
                    self.spawn_with_restart(actor.name().to_string(), worker);
                }
                Ok(())
            }
            Err(e) => {
                error!("Error starting {} : {}", actor.name(), e);
                Err(e)
            }
        }
    }

    fn spawn_with_restart(&mut self, name: String, mut handle: JoinHandle<WorkerResult>) {
        let tasks = &mut self.tasks;
        let task_name = name.clone();
        let task = tokio::spawn(async move {
            let mut backoff = 1;
            loop {
                match &mut handle.await {
                    Ok(Ok(res)) => {
                        info!("ActorWorker {} finished successfully: {:?}", task_name, res);
                        break;
                    }
                    Ok(Err(e)) => {
                        error!("ActorWorker {} finished with error: {:?}", task_name, e);
                    }
                    Err(e) => {
                        error!("ActorWorker {} join error: {:?}", task_name, e);
                    }
                }
                error!("Restarting actor task {} after {} seconds", task_name, backoff);
                sleep(Duration::from_secs(backoff)).await;
                backoff = std::cmp::min(backoff * 2, 60);
                // Here you would restart the actor task by spawning it again
                // This requires access to actor creation logic, which is not available here
                // So this is a placeholder for restart logic
                break; // Remove this break when restart logic is implemented
            }
        });
        tasks.push(task);
    }

    pub fn start_and_wait(&mut self, actor: impl Actor + Send + Sync + 'static) -> Result<()> {
        match actor.start_and_wait() {
            Ok(_) => {
                info!("{} started successfully", actor.name());
                Ok(())
            }
            Err(e) => {
                error!("Error starting {} : {}", actor.name(), e);
                Err(e)
            }
        }
    }

    pub async fn wait(self) {
        let mut f_remaining_futures = self.tasks;
        let mut futures_counter = f_remaining_futures.len();

        while futures_counter > 0 {
            let (result, _index, remaining_futures) = futures::future::select_all(f_remaining_futures).await;
            match result {
                Ok(work_result) => match work_result {
                    Ok(s) => {
                        info!("ActorWorker {_index} finished : {s}")
                    }
                    Err(e) => {
                        error!("ActorWorker {_index} finished with error : {e}")
                    }
                },
                Err(e) => {
                    error!("ActorWorker join error {_index} : {e}")
                }
            }
            f_remaining_futures = remaining_futures;
            futures_counter = f_remaining_futures.len();
        }
    }
}
