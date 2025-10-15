use linked_list::LinkedList;
pub mod linked_list;

fn main() {
    let mut list: LinkedList<i32> = LinkedList::new();
    assert!(list.is_empty());
    assert_eq!(list.get_size(), 0);
    for i in 1..12 {
        list.push_front(i);
    }
    println!("{}", list);
    println!("list size: {}", list.get_size());
    println!("top element: {}", list.pop_front().unwrap());
    println!("{}", list);
    println!("size: {}", list.get_size());
    println!("{}", list.to_string()); // ToString impl for anything impl Display

    // 测试 == 运算符（通过 PartialEq trait 实现）
    println!("\n--- 测试链表相等性判断 (== 运算符) ---");
    
    // 创建两个相同的链表
    let mut list1: LinkedList<i32> = LinkedList::new();
    let mut list2: LinkedList<i32> = LinkedList::new();
    for i in 1..5 {
        list1.push_front(i);
        list2.push_front(i);
    }
    
    println!("list1: {}", list1);
    println!("list2: {}", list2);
    println!("list1 == list2: {}", list1 == list2); // 使用 == 运算符
    
    // 测试不同的链表
    list2.push_front(10);
    println!("\n在 list2 添加元素 10 后:");
    println!("list1: {}", list1);
    println!("list2: {}", list2);
    println!("list1 == list2: {}", list1 == list2); // 使用 == 运算符
    
    // 测试克隆后的链表
    let list3 = list1.clone();
    println!("\nlist3 是 list1 的克隆:");
    println!("list1: {}", list1);
    println!("list3: {}", list3);
    println!("list1 == list3: {}", list1 == list3); // 使用 == 运算符
    
    // 测试 != 运算符（也由 PartialEq 提供）
    println!("\nlist1 != list2: {}", list1 != list2); // 使用 != 运算符

    // If you implement iterator trait:
    //for val in &list {
    //    println!("{}", val);
    //}
}
