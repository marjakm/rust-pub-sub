#[macro_use] extern crate log;
extern crate thread_monitor;

use std::mem;
use std::thread::JoinHandle;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Sender, Receiver};
use thread_monitor::MonitoredThreadSpawner;


pub fn pubsub<T: Clone+Send+'static>(spawner: &MonitoredThreadSpawner) -> (Publisher<T>, Subscriber<T>) {
    let (p_tx, s_rx) = channel();
    let (s_tx, p_rx) = channel();
    let mut pt: PublisherThread<T> = PublisherThread {
        tx: p_tx,
        rx: p_rx,
        subscribers: Vec::new(),
    };
    let p = Publisher {
        tx: s_tx.clone(),
        jh: Arc::new(Mutex::new(Some(
            spawner.spawn_monitored_thread("pubsub publisher", move ||
            pt.main_loop()
        )))),
    };
    let s = Subscriber {
        tx: Arc::new(Mutex::new(s_tx)),
        rx: Arc::new(Mutex::new(s_rx)),
    };
    (p, s)
}

#[derive(Clone)]
pub struct Publisher<T: Clone> {
    tx: Sender<PubReq<T>>,
    jh: Arc<Mutex<Option<JoinHandle<()>>>>
}

impl<T: Clone> Publisher<T> {
    pub fn publish(&mut self, t: T) -> Result<(), String> {
        self.tx.send(PubReq::Publish(t)).map_err(|e| ::std::string::ToString::to_string(&e))
    }

    pub fn stop(&mut self) -> Result<(), String> {
        match self.jh.lock().map(|mut x| x.take()) {
            Ok(Some(jh)) => {
                try!(self.tx.send(PubReq::Stop).map_err(|e| ::std::string::ToString::to_string(&e)));
                jh.join().map_err(|_| "Could not join thread".to_string())
            }
            _ => {
                Err("Join handle error".to_string())
            }
        }
    }
}

#[derive(Clone)]
pub struct Subscriber<T: Clone> {
    tx: Arc<Mutex<Sender<PubReq<T>>>>,
    rx: Arc<Mutex<Receiver<Receiver<T>>>>,
}

impl<T: Clone> Subscriber<T> {
    pub fn subscribe(&self) -> Receiver<T> {
        self.tx.lock().expect("could not lock tx").send(PubReq::Subscribe).expect("Could send subscribe req");
        self.rx.lock().expect("could not lock rx").recv().expect("Could not receive subscribe receive")
    }
}

enum PubReq<T: Clone> {
    Publish(T),
    Subscribe,
    Stop
}

struct PublisherThread<T: Clone> {
    tx: Sender<Receiver<T>>,
    rx: Receiver<PubReq<T>>,
    subscribers: Vec<Sender<T>>,
}

impl<T: Clone> PublisherThread<T> {
    fn main_loop(&mut self) {
        loop {
            match self.rx.recv().expect("Could not receive pubreq") {
                PubReq::Publish(t) => self.publish(t),
                PubReq::Subscribe => self.subscribe(),
                PubReq::Stop => break
            }
        }
    }

    fn subscribe(&mut self) {
        let (t, r) = channel();
        self.tx.send(r).expect("Could not send subscribe receive");
        self.subscribers.push(t);
    }

    fn publish(&mut self, t: T) {
        let subs = mem::replace(&mut self.subscribers, Vec::new());
        for sub in subs.into_iter() {
            match sub.send(t.clone()) {
                Ok(()) => self.subscribers.push(sub),
                Err(e) => warn!("Send error: {}, removing subscription", e)
            };
        }
    }
}
