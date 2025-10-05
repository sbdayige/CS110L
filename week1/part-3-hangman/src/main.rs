// Simple Hangman Program
// User gets five incorrect guesses
// Word chosen randomly from words.txt
// Inspiration from: https://doc.rust-lang.org/book/ch02-00-guessing-game-tutorial.html
// This assignment will introduce you to some fundamental syntax in Rust:
// - variable declaration
// - string manipulation
// - conditional statements
// - loops
// - vectors
// - files
// - user input
// We've tried to limit/hide Rust's quirks since we'll discuss those details
// more in depth in the coming lectures.
extern crate rand;
use rand::Rng;
use std::fs;
use std::io;
use std::io::Write;

const NUM_INCORRECT_GUESSES: u32 = 5;
const WORDS_PATH: &str = "words.txt";

fn pick_a_random_word() -> String {
    let file_string = fs::read_to_string(WORDS_PATH).expect("Unable to read file.");
    let words: Vec<&str> = file_string.split('\n').collect();
    String::from(words[rand::thread_rng().gen_range(0, words.len())].trim())
}

// Read a single valid letter from stdin. Re-prompts until the user
// enters exactly one alphabetic character. Returns the lowercase char.
fn read_guess() -> char {
    loop {
        print!("Please guess a letter: ");
        io::stdout().flush().expect("Error flushing stdout.");
        let mut guess = String::new();
        io::stdin()
            .read_line(&mut guess)
            .expect("Error reading line.");
        let guess = guess.trim();

        if guess.len() != 1 {
            println!("Please enter exactly one character.");
            continue;
        }

        let ch = guess.chars().next().unwrap();
        if !ch.is_alphabetic() {
            println!("Please enter a letter (a-z) || (A-Z).",);
            continue;
        }
        println!();
        return ch.to_ascii_lowercase();
    }
}

fn main() {
    let secret_word = pick_a_random_word();
    let secret_word_chars: Vec<char> = secret_word.chars().collect();
    let mut have_guessed: Vec<char> = vec![];
    println!("Welcome to CS110L Hangman!");
    let mut guessed_word: Vec<char> = vec!['_'; secret_word.len()];
    let mut can_guesses = NUM_INCORRECT_GUESSES;
    while guessed_word != secret_word_chars && can_guesses > 0 {
        println!("The word so far is {:?}", guessed_word);
        println!("You have guessed the following letters: {:?}", have_guessed);
        println!("You have {} guesses left", can_guesses);
        let guess_char = read_guess();
        have_guessed.push(guess_char);
        let mut flag = false;
        for i in 0..secret_word.len() {
            if secret_word_chars[i] == guess_char && guessed_word[i] == '_' {
                guessed_word[i] = guess_char;
                flag = true;
                break;
            }
        }
        if !flag {
            can_guesses -= 1;
        }
    }

    if guessed_word == secret_word_chars {
        println!(
            "Congratulations you guessed the secret word: {:?}!",
            guessed_word
        );
    } else {
        println!("Sorry, you ran out of guesses!");
    }
}
