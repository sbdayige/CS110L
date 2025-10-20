use std::pin::Pin;
use std::marker::PhantomPinned;

struct Solution;

impl Solution {
    pub fn min_operations(n: i32) -> i32 {
        let mut v = Vec::new();
        for i in 0..n {
            v.push(2 * i + 1);
        }
        
        
        0
    }
}



fn main() {

}
