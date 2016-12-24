use serde::ser::Serialize;
use serde::de::Deserialize;
use serde_json;
use rdkafka::config::TopicConfig;
use rdkafka::producer::{FutureProducer, EmptyProducerContext, FutureProducerTopic};
use rdkafka::config::ClientConfig;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, RwLock};

use error::*;

type ReplicatorProducer = FutureProducer<EmptyProducerContext>;
type ReplicatorTopic = FutureProducerTopic<EmptyProducerContext>;

pub struct Replicator {
//    brokers: String,
//    topic_name: String,
    producer_topic: Arc<ReplicatorTopic>,
}

impl Replicator {
    pub fn new(brokers: &str, topic_name: &str) -> Result<Replicator> {
        let producer = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            .create::<FutureProducer<_>>()
            .expect("Producer creation error");

        producer.start();

        let topic = producer.get_topic(topic_name, &TopicConfig::new())
            .expect("Topic creation error");

        let replicator = Replicator {
//            brokers: brokers.to_owned(),
//            topic_name: topic_name.to_owned(),
            producer_topic: Arc::new(topic),
        };

        Ok(replicator)
    }

    pub fn create_cache<C: ReplicatedCache>(&self, name: &str) -> C {
        C::new(self.producer_topic.clone(), name)
    }
}

pub trait ReplicatedCache {
    type Key: Serialize + Deserialize;
    type Value: Serialize + Deserialize;

    fn new(Arc<ReplicatorTopic>, &str) -> Self;
    fn name(&self) -> &str;
    fn insert(&self, Self::Key, Self::Value) -> Result<Arc<Self::Value>>;
    fn get(&self, &Self::Key) -> Option<Arc<Self::Value>>;
    fn keys(&self) -> Vec<Self::Key>;
}

// TODO: add name to key
// TODO: use structure for value
fn write_update<K, V>(topic: &ReplicatorTopic, name: &str, key: &K, value: &V) -> Result<()>
        where K: Serialize + Deserialize,
              V: Serialize + Deserialize {
    let serialized_key = serde_json::to_vec(&key)
        .chain_err(|| "Failed to serialize key")?;
    let serialized_value = serde_json::to_vec(&value)
        .chain_err(|| "Failed to serialize value")?;
    let _f = topic.send_copy(None, Some(&serialized_value), Some(&serialized_key))
        .chain_err(|| "Failed to produce message")?;
    // _f.wait();  // Uncomment to make production synchronous
    Ok(())
}

// TODO? use inner object with one Arc?
#[derive(Clone)]
pub struct Cache<K, V>
  where K: Eq + Hash + Clone + Serialize + Deserialize,
        V: Serialize + Deserialize {
    name: String,
    cache_lock: Arc<RwLock<HashMap<K, Arc<V>>>>,
    replicator_topic: Arc<ReplicatorTopic>,
}

impl<K, V> ReplicatedCache for Cache<K, V>
    where K: Eq + Hash + Clone + Serialize + Deserialize,
          V: Serialize + Deserialize {
    type Key = K;
    type Value = V;

    fn new(replicator_topic: Arc<ReplicatorTopic>, name: &str) -> Cache<K, V> {
        Cache {
            name: name.to_owned(),
            cache_lock: Arc::new(RwLock::new(HashMap::new())),
            replicator_topic: replicator_topic,
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn keys(&self) -> Vec<K> {
        match self.cache_lock.read() {
            Ok(ref cache) => (*cache).keys().cloned().collect::<Vec<_>>(),
            Err(_) => panic!("Poison error"),
        }
    }

    fn insert(&self, key: K, value: V) -> Result<Arc<V>> {
        write_update(self.replicator_topic.as_ref(), &self.name, &key, &value)
            .chain_err(|| "Failed to write cache update")?;
        let value_arc = Arc::new(value);
        match self.cache_lock.write() {
            Ok(mut cache) => (*cache).insert(key, value_arc.clone()),
            Err(_) => panic!("Poison error"),
        };
        Ok(value_arc.clone())
    }

    fn get(&self, key: &K) -> Option<Arc<V>> {
        let value = match self.cache_lock.read() {
            Ok(cache) => (*cache).get(key).map(|arc| arc.clone()),
            Err(_) => panic!("Poison error"),
        };
        value
    }
}

// pub struct Cache<K, V>
//   where K: Eq + Hash + Serialize + Deserialize,
//         V: Serialize + Deserialize {
//     cache_lock: Arc<RwLock<HashMap<K, V>>>,
//     on_insert: Option<Box<Fn(&K, &V)>>,
//     on_delete: Option<Box<Fn(&K, &V)>>
// }
//
// impl<K, V> Cache<K, V>
//   where K: Eq + Hash + Serialize + Deserialize,
//         V: Serialize + Deserialize {
//
//     pub fn new() -> Cache<K, V> {
//         Cache {
//             cache_lock: Arc::new(RwLock::new(HashMap::new())),
//             on_insert: None,
//             on_delete: None,
//         }
//     }
//
//     pub fn set_on_insert<'a, CB: 'static + Fn(&K, &V)>(&'a mut self, cb: CB) -> &'a mut Cache<K, V> {
//         self.on_insert = Some(Box::new(cb));
//         self
//     }
//
//     pub fn insert(&self, key: K, value: V) {
//         self.on_insert.as_ref().map(|f| (f)(&key, &value));
//         match self.cache_lock.write() {
//             Ok(mut cache_ref) => (*cache_ref).insert(key, value),
//             Err(_) => panic!("Poison error!"),
//         };
//     }
// }