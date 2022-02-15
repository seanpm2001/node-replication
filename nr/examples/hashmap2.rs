// Copyright © 2019-2020 VMware, Inc. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! A minimal example that implements a replicated hashmap
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;

use node_replication::Dispatch;
use node_replication::NodeReplicated;

/// The node-replicated hashmap uses a std hashmap internally.
#[derive(Default)]
struct NrHashMap {
    storage: HashMap<u64, u64>,
}

/// We support mutable put operation on the hashmap.
#[derive(Clone, Debug, PartialEq)]
enum Modify {
    Put(u64, u64),
}

/// We support an immutable read operation to lookup a key from the hashmap.
#[derive(Clone, Debug, PartialEq)]
enum Access {
    Get(u64),
}

/// The Dispatch traits executes `ReadOperation` (our Access enum)
/// and `WriteOperation` (our `Modify` enum) against the replicated
/// data-structure.
impl Dispatch for NrHashMap {
    type ReadOperation = Access;
    type WriteOperation = Modify;
    type Response = Option<u64>;

    /// The `dispatch` function applies the immutable operations.
    fn dispatch(&self, op: Self::ReadOperation) -> Self::Response {
        match op {
            Access::Get(key) => self.storage.get(&key).map(|v| *v),
        }
    }

    /// The `dispatch_mut` function applies the mutable operations.
    fn dispatch_mut(&mut self, op: Self::WriteOperation) -> Self::Response {
        match op {
            Modify::Put(key, value) => self.storage.insert(key, value),
        }
    }
}

/// We initialize a log, and two replicas for a hashmap, register with the replica
/// and then execute operations.
fn main() {
    let _r = env_logger::try_init();
    const PER_THREAD_OPS: u64 = 2_948_048;
    // Next, we create the replicated the hashmap
    for replica_cntr in 1..=4 {
        let num_replica = NonZeroUsize::new(replica_cntr).unwrap();
        let nrht = Arc::new(NodeReplicated::<NrHashMap>::new(num_replica, |_rid| {}).unwrap());

        // The replica executes a Modify or Access operations by calling
        // `execute_mut` and `execute`. Eventually they end up in the `Dispatch` trait.
        let thread_loop = |replica: Arc<NodeReplicated<NrHashMap>>, ttkn| {
            for i in 0..PER_THREAD_OPS {
                let _r = match i % 2 {
                    0 => replica.execute_mut(Modify::Put(i, i + 1), ttkn),
                    1 => {
                        let response = replica.execute(Access::Get(i - 1), ttkn);
                        assert_eq!(response, Some(i));
                        response
                    }
                    _ => unreachable!(),
                };
            }
        };

        let now = std::time::Instant::now();
        // Finally, we spawn three threads that issue operations, thread 1, 2, 3
        // will use replicas 1, 2, 3 -- 4th replica will implicitly be served by others
        // because we can in this model...
        for thread_num in 1..=4 {
            print!(
                "Running with {} replicas and {} threads",
                replica_cntr, thread_num
            );

            let mut threads = Vec::with_capacity(thread_num);
            for t in 0..thread_num {
                let nrht_cln = nrht.clone();
                threads.push(std::thread::spawn(move || {
                    let ttkn = nrht_cln.register(t % replica_cntr).expect(
                        format!(
                            "Unable to register thread with replica {}",
                            t % replica_cntr
                        )
                        .as_str(),
                    );
                    thread_loop(nrht_cln, ttkn);
                }));
            }

            // Wait for all the threads to finish
            for thread in threads {
                thread.join().unwrap();
            }

            println!(
                " ({} ns/op)",
                now.elapsed().as_nanos() / (thread_num as u128 * PER_THREAD_OPS as u128)
            );
        }
    }
}
