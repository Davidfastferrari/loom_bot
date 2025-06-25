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

    pub fn start<F>(&mut self, actor_factory: F) -> Result<()>
    where
        F: Fn() -> Box<dyn Actor + Send + Sync> + Send + Sync + 'static + Clone,
    {
        let actor = actor_factory();
        let actor_name = actor.name().to_string();
        match actor.start() {
            Ok(workers) => {
                info!("{} started successfully", actor_name);
                for worker in workers {
                    self.spawn_with_restart(actor_name.clone(), worker, actor_factory.clone());
                }
                Ok(())
            }
            Err(e) => {
                error!("Error starting {} : {}", actor_name, e);
                Err(e)
            }
        }
    }

    fn spawn_with_restart<F>(&mut self, name: String, mut handle: JoinHandle<WorkerResult>, actor_factory: F)
    where
        F: Fn() -> Box<dyn Actor + Send + Sync> + Send + Sync + 'static + Clone,
    {
        let tasks = &mut self.tasks;
        let task_name = name.clone();
        let factory = actor_factory.clone();
        let task = tokio::spawn(async move {
            let mut backoff = 1;
            loop {
                match &mut handle.await {
                    Ok(res) => {
                        info!("ActorWorker {} finished successfully: {:?}", task_name, res);
                        break;
                    }
                    Err(e) => {
                        error!("ActorWorker {} join error: {:?}", task_name, e);
                    }
                }
                error!("Restarting actor task {} after {} seconds", task_name, backoff);
                sleep(Duration::from_secs(backoff)).await;
                backoff = std::cmp::min(backoff * 2, 60);
                // Restart the actor task by spawning it again
                let new_actor = factory();
                match new_actor.start() {
                    Ok(new_workers) => {
                        info!("{} restarted successfully", task_name);
                        if let Some(new_worker) = new_workers.into_iter().next() {
                            handle = new_worker;
                            continue;
                        } else {
                            error!("{} restart failed: no worker returned", task_name);
                            break;
                        }
                    }
                    Err(e) => {
                        error!("{} restart failed: {}", task_name, e);
                        break;
                    }
                }
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
