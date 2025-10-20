use crossbeam_channel;
use std::{thread, time};

fn parallel_map<T, U, F>(input_vec: Vec<T>, num_threads: usize, f: F) -> Vec<U>
where
    F: FnOnce(T) -> U + Send + Copy + 'static,
    T: Send + 'static,
    U: Send + 'static + Default,
{
    let mut output_vec: Vec<U> = Vec::with_capacity(input_vec.len());
    output_vec.resize_with(input_vec.len(), Default::default);
    
    // 创建用于发送任务的通道
    let (task_sender, task_receiver) = crossbeam_channel::unbounded::<(usize, T)>();
    
    // 创建用于接收结果的通道
    let (result_sender, result_receiver) = crossbeam_channel::unbounded::<(usize, U)>();
    
    // 创建工作线程池
    let mut handles = Vec::new();
    for _ in 0..num_threads {
        let task_rx = task_receiver.clone();
        let result_tx = result_sender.clone();
        
        let handle = thread::spawn(move || {
            // 每个线程不断从任务通道接收任务
            while let Ok((index, item)) = task_rx.recv() {
                let result = f(item);
                // 将结果连同索引发送回主线程
                result_tx.send((index, result)).unwrap();
            }
        });
        
        handles.push(handle);
    }
    
    // 丢弃主线程持有的 result_sender，这样当所有工作线程完成后通道会关闭
    drop(result_sender);
    
    // 将所有任务发送到任务通道
    for (index, item) in input_vec.into_iter().enumerate() {
        task_sender.send((index, item)).unwrap();
    }
    
    // 关闭任务通道，让工作线程知道没有更多任务了
    drop(task_sender);
    
    // 接收所有结果
    while let Ok((index, result)) = result_receiver.recv() {
        output_vec[index] = result;
    }
    
    // 等待所有线程完成
    for handle in handles {
        handle.join().unwrap();
    }
    
    output_vec
}

fn main() {
    let v = vec![6, 7, 8, 9, 10, 1, 2, 3, 4, 5, 12, 18, 11, 5, 20];
    let squares = parallel_map(v, 10, |num| {
        println!("{} squared is {}", num, num * num);
        thread::sleep(time::Duration::from_millis(500));
        num * num
    });
    println!("squares: {:?}", squares);
}
