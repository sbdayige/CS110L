use std::fmt;
use std::option::Option;

pub struct LinkedList<T> {
    head: Option<Box<Node<T>>>,
    size: usize,
}

struct Node<T> {
    value: T,
    next: Option<Box<Node<T>>>,
}

impl<T: Clone + PartialEq> Node<T> {
    pub fn new(value: T, next: Option<Box<Node<T>>>) -> Node<T> {
        Node {value: value, next: next}
    }
}

impl<T: Clone + PartialEq> Clone for Node<T> {
    fn clone(&self) -> Node<T> {
        Node {
            value: self.value.clone(),
            next: self.next.clone(),
        }
    }
}

impl<T: Clone + PartialEq> LinkedList<T> {
    pub fn new() -> LinkedList<T> {
        LinkedList {head: None, size: 0}
    }
    
    pub fn get_size(&self) -> usize {
        self.size
    }
    
    pub fn is_empty(&self) -> bool {
        self.get_size() == 0
    }
    
    pub fn push_front(&mut self, value: T) {
        let new_node: Box<Node<T>> = Box::new(Node::new(value, self.head.take()));
        self.head = Some(new_node);
        self.size += 1;
    }
    
    pub fn pop_front(&mut self) -> Option<T> {
        let node: Box<Node<T>> = self.head.take()?;
        self.head = node.next;
        self.size -= 1;
        Some(node.value)
    }
    
    /// Returns a reference to the first element, or None if empty
    pub fn peek(&self) -> Option<&T> {
        self.head.as_ref().map(|node| &node.value)
    }
    
    /// Returns a mutable reference to the first element, or None if empty
    pub fn peek_mut(&mut self) -> Option<&mut T> {
        self.head.as_mut().map(|node| &mut node.value)
    }
    
    /// Clears the list, removing all elements
    pub fn clear(&mut self) {
        self.head = None;
        self.size = 0;
    }
    
    /// Converts the list to a Vec
    pub fn to_vec(&self) -> Vec<T> {
        let mut vec = Vec::new();
        let mut current = &self.head;
        while let Some(node) = current {
            vec.push(node.value.clone());
            current = &node.next;
        }
        vec
    }
    
    /// Creates a LinkedList from a Vec
    pub fn from_vec(vec: Vec<T>) -> LinkedList<T> {
        let mut list = LinkedList::new();
        for item in vec.into_iter().rev() {
            list.push_front(item);
        }
        list
    }
}

impl<T: Clone + PartialEq> Clone for LinkedList<T> {
    fn clone(&self) -> LinkedList<T> {
        let mut new_list = LinkedList::new();
        new_list.head = self.head.clone();
        new_list.size = self.size;
        new_list
    }
}

impl<T: Clone + PartialEq> PartialEq for LinkedList<T> {
    fn eq(&self, other: &Self) -> bool {
        // First check if sizes are equal
        if self.size != other.size {
            return false;
        }
        
        // Then compare each node
        let mut current_self = &self.head;
        let mut current_other = &other.head;
        
        while let (Some(node_self), Some(node_other)) = (current_self, current_other) {
            if node_self.value != node_other.value {
                return false;
            }
            current_self = &node_self.next;
            current_other = &node_other.next;
        }
        
        // Both should be None at this point
        true
    }
}

impl<T: Clone + PartialEq + fmt::Debug> fmt::Debug for LinkedList<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.to_vec().iter()).finish()
    }
}

impl<T: fmt::Display> fmt::Display for LinkedList<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut current: &Option<Box<Node<T>>> = &self.head;
        let mut result = String::new();
        loop {
            match current {
                Some(node) => {
                    result = format!("{} {}", result, node.value);
                    current = &node.next;
                },
                None => break,
            }
        }
        write!(f, "{}", result)
    }
}

impl<T> Drop for LinkedList<T> {
    fn drop(&mut self) {
        let mut current = self.head.take();
        while let Some(mut node) = current {
            current = node.next.take();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_list() {
        let list: LinkedList<i32> = LinkedList::new();
        assert!(list.is_empty());
        assert_eq!(list.get_size(), 0);
    }

    #[test]
    fn test_push_and_pop() {
        let mut list: LinkedList<i32> = LinkedList::new();
        list.push_front(1);
        list.push_front(2);
        list.push_front(3);
        
        assert_eq!(list.get_size(), 3);
        assert_eq!(list.pop_front(), Some(3));
        assert_eq!(list.pop_front(), Some(2));
        assert_eq!(list.pop_front(), Some(1));
        assert_eq!(list.pop_front(), None);
        assert!(list.is_empty());
    }

    #[test]
    fn test_equality_same_lists() {
        let mut list1: LinkedList<i32> = LinkedList::new();
        let mut list2: LinkedList<i32> = LinkedList::new();
        
        for i in 1..5 {
            list1.push_front(i);
            list2.push_front(i);
        }
        
        // 测试 == 运算符
        assert!(list1 == list2);
        assert_eq!(list1, list2);
    }

    #[test]
    fn test_equality_different_length() {
        let mut list1: LinkedList<i32> = LinkedList::new();
        let mut list2: LinkedList<i32> = LinkedList::new();
        
        list1.push_front(1);
        list1.push_front(2);
        
        list2.push_front(1);
        list2.push_front(2);
        list2.push_front(3);
        
        // 测试 != 运算符
        assert!(list1 != list2);
        assert_ne!(list1, list2);
    }

    #[test]
    fn test_equality_different_values() {
        let mut list1: LinkedList<i32> = LinkedList::new();
        let mut list2: LinkedList<i32> = LinkedList::new();
        
        list1.push_front(1);
        list1.push_front(2);
        list1.push_front(3);
        
        list2.push_front(1);
        list2.push_front(2);
        list2.push_front(4); // 不同的值
        
        // 测试 != 运算符
        assert!(list1 != list2);
        assert_ne!(list1, list2);
    }

    #[test]
    fn test_equality_empty_lists() {
        let list1: LinkedList<i32> = LinkedList::new();
        let list2: LinkedList<i32> = LinkedList::new();
        
        // 两个空链表应该相等
        assert!(list1 == list2);
        assert_eq!(list1, list2);
    }

    #[test]
    fn test_clone() {
        let mut list1: LinkedList<i32> = LinkedList::new();
        for i in 1..5 {
            list1.push_front(i);
        }
        
        let list2 = list1.clone();
        
        // 克隆的链表应该相等
        assert_eq!(list1, list2);
        assert_eq!(list1.get_size(), list2.get_size());
    }

    #[test]
    fn test_clone_independence() {
        let mut list1: LinkedList<i32> = LinkedList::new();
        list1.push_front(1);
        list1.push_front(2);
        
        let mut list2 = list1.clone();
        
        // 修改 list2 不应该影响 list1
        list2.push_front(3);
        
        assert_ne!(list1, list2);
        assert_eq!(list1.get_size(), 2);
        assert_eq!(list2.get_size(), 3);
    }

    #[test]
    fn test_peek() {
        let mut list: LinkedList<i32> = LinkedList::new();
        assert_eq!(list.peek(), None);
        
        list.push_front(1);
        list.push_front(2);
        
        assert_eq!(list.peek(), Some(&2));
        assert_eq!(list.get_size(), 2); // peek 不应该改变大小
    }

    #[test]
    fn test_peek_mut() {
        let mut list: LinkedList<i32> = LinkedList::new();
        list.push_front(1);
        list.push_front(2);
        
        if let Some(value) = list.peek_mut() {
            *value = 10;
        }
        
        assert_eq!(list.peek(), Some(&10));
    }

    #[test]
    fn test_clear() {
        let mut list: LinkedList<i32> = LinkedList::new();
        for i in 1..5 {
            list.push_front(i);
        }
        
        list.clear();
        
        assert!(list.is_empty());
        assert_eq!(list.get_size(), 0);
        assert_eq!(list.peek(), None);
    }

    #[test]
    fn test_to_vec() {
        let mut list: LinkedList<i32> = LinkedList::new();
        list.push_front(3);
        list.push_front(2);
        list.push_front(1);
        
        let vec = list.to_vec();
        assert_eq!(vec, vec![1, 2, 3]);
    }

    #[test]
    fn test_from_vec() {
        let vec = vec![1, 2, 3];
        let list = LinkedList::from_vec(vec);
        
        assert_eq!(list.get_size(), 3);
        assert_eq!(list.to_vec(), vec![1, 2, 3]);
    }

    #[test]
    fn test_from_vec_and_equality() {
        let mut list1: LinkedList<i32> = LinkedList::new();
        list1.push_front(3);
        list1.push_front(2);
        list1.push_front(1);
        
        let list2 = LinkedList::from_vec(vec![1, 2, 3]);
        
        // 使用 == 运算符测试相等性
        assert_eq!(list1, list2);
    }

    #[test]
    fn test_with_strings() {
        let mut list1: LinkedList<String> = LinkedList::new();
        list1.push_front(String::from("world"));
        list1.push_front(String::from("hello"));
        
        let mut list2: LinkedList<String> = LinkedList::new();
        list2.push_front(String::from("world"));
        list2.push_front(String::from("hello"));
        
        // 测试字符串类型的链表相等性
        assert_eq!(list1, list2);
    }
}



