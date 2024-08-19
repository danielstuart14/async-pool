//! This example demonstrates the use of the `async_pool` crate with the `tokio` framework.

use tokio::time::sleep;
use tokio::join;

use std::mem;
use std::time::Duration;

use async_pool::{pool, Box};

#[derive(Debug)]
#[allow(dead_code)]
struct Packet(u32);

// A maximum of 2 Packet instances can be allocated at a time.
// A maximum of 1 future can be waiting at a time.
pool!(PacketPool: [Packet; 2], 1);

#[tokio::main]
async fn main() {
    // Allocate non-blocking
    let fut1 = async {
        println!("1 - allocating async...");
        let box1 = Box::<PacketPool>::new(Packet(1));
        println!("1 - allocated: {:?}", box1);
        sleep(Duration::from_millis(100)).await;
        println!("1 - dropping allocation...");
        mem::drop(box1);
    };

    // Allocate asynchronously
    let fut2 = async {
        sleep(Duration::from_millis(5)).await;
        println!("2 - allocating sync...");
        let box2 = Box::<PacketPool>::new_async(Packet(2)).await;
        println!("2 - allocated: {:?}", box2);
        sleep(Duration::from_millis(150)).await;
        println!("2 - dropping allocation...");
        mem::drop(box2);
    };

    // Allocate non-blocking (fails, data pool is full)
    let fut3 = async {
        sleep(Duration::from_millis(10)).await;
        println!("3 - allocating sync...");
        let box3 = Box::<PacketPool>::new(Packet(3));
        println!(
            "3 - allocation fails because the data pool is full: {:?}",
            box3
        );
    };

    // Allocate asynchronously (waits for a deallocation)
    let fut4 = async {
        sleep(Duration::from_millis(15)).await;
        println!("4 - allocating async...");
        let box4 = Box::<PacketPool>::new_async(Packet(4)).await;
        println!("4 - allocated: {:?}", box4);
        sleep(Duration::from_millis(100)).await;
        println!("4 - dropping allocation...");
    };

    // Allocate asynchronously (fails, waker pool is full)
    let fut5 = async {
        sleep(Duration::from_millis(20)).await;
        println!("5 - allocating async...");
        let box5 = Box::<PacketPool>::new_async(Packet(5)).await;
        println!(
            "5 - allocation fails because the waker pool is full: {:?}",
            box5
        );
    };

    join!(fut1, fut2, fut3, fut4, fut5);
}