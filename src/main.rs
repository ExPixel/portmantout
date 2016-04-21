extern crate rand;
extern crate toml;

use std::ascii::AsciiExt;

use std::io::prelude::*;
use std::fs::File;
use std::io::BufReader;
use std::thread;
use std::sync::{Arc, RwLock};
use std::mem;
use std::sync::mpsc;

pub const SMALL_WORD_LIST: &'static str = "words.txt";
pub const LARGE_WORD_LIST: &'static str = "words-large.txt";

pub const SMALL_WORD_LIST_EXT: &'static str = "words-ext.txt";
pub const LARGE_WORD_LIST_EXT: &'static str = "words-large-ext.txt";

pub struct ProgramOptions {
	minimum_overlap: usize,
	threads: usize,
	save_extensions: bool,
	verbose: bool,
	max_word_chain: u64,
	allow_word_reuse: bool,
	use_large_words_file: bool
}

macro_rules! task {
	($name:expr, $task:block) => ({
		println!("[START] {}", $name);
		let _ret = $task;
		println!("[END] {}", $name);
		_ret
	})
}

macro_rules! get_percentage {
	($complete:expr, $total:expr) => ({
		let complete_fp = $complete as f64;
		let total_fp = $total as f64;
		complete_fp / total_fp * 100f64
	})
}

struct PortmantoutState {
	chain: i64,
	max_chain: i64
}

impl PortmantoutState {
	fn new(max: i64) -> PortmantoutState {
		PortmantoutState {
			chain: 0,
			max_chain: max
		}
	}

	fn inc_chain(&mut self) {
		self.chain += 1
	}

	fn at_max(&self) -> bool {
		self.chain >= self.max_chain
	}
}

struct Word {
	value: String,
	extensions: Option<Vec<u32>>,
	used: bool
}

fn trim_string(s: &str) -> String {
	String::from(s.trim())
}

fn read_words_from_file(file: File) -> Vec<Word> {
	let mut words = Vec::new();
	let buf = BufReader::new(file);

	for l in buf.lines() {
		if let Ok(line) = l {
			if line.len() > 0 {
				let word = Word {
					value: trim_string(&line),
					extensions: None,
					used: false
				};
				words.push(word);
			}
		}
	}

	return words
}

fn read_words_and_ext_from_file(file: File) -> Vec<Word> {
	let mut words = Vec::new();
	let buf = BufReader::new(file);

	for l in buf.lines() {
		if let Ok(line) = l {
			if line.len() > 0 {
				if let Some(word) = create_word_and_ext_from_line(line) {
					words.push(word);
				}
			}
		}
	}

	return words
}

fn create_word_and_ext_from_line(line: String) -> Option<Word> {
	let mut split = line.split_whitespace();
	let mut word_value = None;
	let mut extensions: Vec<u32> = Vec::new();

	if let Some(wv) = split.next() {
		word_value = Some(String::from(wv));
		for ext_str in split {
			if let Ok(ext_idx) = ext_str.parse::<u32>() {
				extensions.push(ext_idx);
			} else {
				println!("discarded word '{}' due to bad parse of ext '{}'.", wv, ext_str);
				return None
			}
		}
	}

	return if let Some(word_value) = word_value {
		Some(Word {
			value: word_value,
			extensions: Some(extensions),
			used: false
		})
	} else {
		None
	};
}

fn find_all_extensions(mut words: Vec<Word>, options: &ProgramOptions) -> Vec<Word> {
	let display_step = if cfg!(feature = "large") {
		100
	} else {
		words.len() / 100
	};
	let mut next_display = display_step;

	let _num_threads = options.threads;
	let _len = words.len();

	let sneaky_words: *mut Vec<Word> = &mut words;
	let shared_words = unsafe { Arc::new(RwLock::new(mem::transmute::<_, &mut Vec<Word>>(sneaky_words))) };
	let fuck = shared_words.clone();

	let (tx, rx) = mpsc::channel();

	// let shared_tx = Arc::new(tx);


	let mut idx_start = 0;
	while idx_start < _len {
		let shared_words_clone = shared_words.clone();
		let ctx = tx.clone();
		// let shared_tx_clone = shared_tx.clone();
		thread::spawn(move || {
			let mut _max = idx_start + (_len / _num_threads);
			if _max > _len { _max = _len }
			for idx in idx_start.._max {
				let _lock = shared_words_clone.read().unwrap();
				let result = Arc::new(find_extensions_for(idx, &_lock));
				ctx.clone().send(result).expect("Failed to send result.");
			}
		});
		idx_start += words.len() / _num_threads;
	}

	let mut completed = 0;

	loop {
		if let Ok(data) = rx.recv() {
			let data_clone = data.clone();
			let mut _lock = fuck.write().unwrap();
			_lock[data_clone.0].extensions = Some(data_clone.1.clone());
			completed += 1;

			if completed >= next_display {
				println!("{} / {} words ({:.2}%)", completed, _len, get_percentage!(completed, words.len()));
				next_display += display_step;
			}

			if completed >= _len { // We don't need to wait anymore in this case.
				break;
			}
		} else {
			break;
		}
	}

	mem::forget(shared_words);

	return words
}

fn find_extensions_for(word_idx: usize, words: &Vec<Word>) -> (usize, Vec<u32>) {
	// if words[word_idx].value.len() < PORT_MIN_OVERLAP { return }
	let mut extensions = Vec::new();
	for idx in 0..words.len() {
		if idx == word_idx { continue }
		// if words[idx].value.len() < PORT_MIN_OVERLAP { continue }

		// #TODO this is much faster if it's done backwards.
		for check_start in 0..words[word_idx].value.len() {
			if words[idx].value.starts_with(&words[word_idx].value[check_start..]) {
				extensions.push(idx as u32);
				break
			}
		}
	}
	return (word_idx, extensions)
}

fn write_words_and_extensions_to_file(mut file: File, words: &Vec<Word>) {
	let display_step = if cfg!(feature = "large") {
		100
	} else {
		words.len() / 100
	};

	let mut next_display = display_step;

	let mut buffer = String::new();
	for word_idx in 0..words.len() {
		buffer.clear();
		let word = &words[word_idx];
		buffer.push_str(&word.value);
		if word.extensions.is_some() {
			for ext_idx in 0..word.extensions.as_ref().unwrap().len() {
				let ext = word.extensions.as_ref().unwrap()[ext_idx];
				buffer.push(' ');
				buffer.push_str(&ext.to_string());
			}
		}
		buffer.push('\n');
		file.write(&buffer.as_bytes()).expect("Unable to write to file.");

		if (word_idx + 1) >= next_display {
			println!("{} / {} words ({:.2}%)", word_idx + 1, words.len(), get_percentage!(word_idx + 1, words.len()));
			next_display += display_step;
		}
	}
}

fn append_portmantout_word(last_word: &String, new_word: &String, pbuf: &mut String, options: &ProgramOptions) -> bool {
	let mut plength = 0;
	let mut matched_plength = 0;
	let mut is_match = false;
	
	while plength < last_word.len() && plength < new_word.len() {
		let last_word_sub = &last_word[(last_word.len() - (plength + 1))..];
		let new_word_sub = &new_word[0..(plength + 1)];
		plength += 1;
		if last_word_sub.eq_ignore_ascii_case(new_word_sub) {
			is_match = true;
			matched_plength = plength;
		}
	}

	if is_match {
		if matched_plength < new_word.len() {
			let new_word_sub = &new_word[matched_plength..];
			if new_word_sub.len() >= options.minimum_overlap {
				if options.verbose {
					println!("added {} ({})", new_word_sub, new_word);
				}
				pbuf.push_str(new_word_sub);
				return true
			}
		}
	}

	return false
}

fn build_portmantout(last_word_idx: usize, words: &mut Vec<Word>, pbuf: &mut String, state: &mut PortmantoutState, options: &ProgramOptions) -> Option<usize> {
	if words[last_word_idx].extensions.is_some() && words[last_word_idx].extensions.as_ref().unwrap().len() < 1 { return None }
	else if state.at_max() { return None }
	let r_rand_idx = rand::random::<usize>() % words[last_word_idx].extensions.as_ref().unwrap().len();

	for r_idx_offset in 0..words[last_word_idx].extensions.as_ref().unwrap().len() {
		let r_idx = (r_rand_idx + r_idx_offset) % words[last_word_idx].extensions.as_ref().unwrap().len();
		let extension = words[last_word_idx].extensions.as_ref().unwrap()[r_idx] as usize;
		if (options.allow_word_reuse || !words[extension].used) && append_portmantout_word(&words[last_word_idx].value, &words[extension].value, pbuf, options) {
			words[extension].used = true;
			state.inc_chain();
			return Some(extension);
		}
	}

	return None
}

fn load_options() -> ProgramOptions {
	let mut toml_file = File::open("settings.toml").expect("Unable to read settings.toml");
	let mut toml = String::new();
	toml_file.read_to_string(&mut toml).expect("Unable to read settings file into string.");
	let table = toml::Parser::new(&toml).parse().unwrap();

	let minimum_overlap = table.get("minimum-overlap")
		.expect("could not find value 'minimum-overlap' in settings").as_integer()
		.expect("Could not parse value 'minimum-overlap' correctly.");
	let threads = table.get("thread")
		.expect("could not find value 'thread' in settings").as_integer()
		.expect("Could not parse value 'thread' correctly.");
	let save_extensions = table.get("save-extensions")
		.expect("could not find value 'save-extensions' in settings").as_bool()
		.expect("Could not parse value 'save-extensions' correctly.");
	let max_word_chain = table.get("max-word-chain")
		.expect("could not find value 'max-word-chain' in settings").as_integer()
		.expect("Could not parse value 'max-word-chain' correctly.");
	let verbose = table.get("verbose")
		.expect("could not find value 'verbose' in settings").as_bool()
		.expect("Could not parse value 'verbose' correctly.");
	let allow_word_reuse = table.get("allow-word-reuse")
		.expect("could not find value 'allow-word-reuse' in settings").as_bool()
		.expect("Could not parse value 'allow-word-reuse' correctly.");
	let use_large_words_file = table.get("use-large-words-file")
		.expect("could not find value 'use-large-words-file' in settings").as_bool()
		.expect("Could not parse value 'use-large-words-file' correctly.");

	return ProgramOptions {
		minimum_overlap: minimum_overlap as usize,
		threads: threads as usize,
		save_extensions: save_extensions,
		verbose: verbose,
		max_word_chain: max_word_chain as u64,
		allow_word_reuse: allow_word_reuse,
		use_large_words_file: use_large_words_file
	}
}


fn main() {
	let options = load_options();

	println!("\n=== Portmantout Settings ===");
	println!("Minimum Overlap: {}", options.minimum_overlap);
	println!("Threads: {}", options.threads);
	println!("Save Extensions: {}", options.save_extensions);
	println!("Verbose: {}", options.verbose);
	println!("Max Word Chain: {}", options.max_word_chain);
	println!("Allow Word Reuse: {}", options.allow_word_reuse);
	println!("USE LARGE WORDS FILE: {}", options.use_large_words_file);
	println!("============================\n");


	let words_file_path = if options.use_large_words_file { LARGE_WORD_LIST } else { SMALL_WORD_LIST };
	let words_ext_file_path = if options.use_large_words_file { LARGE_WORD_LIST_EXT } else { SMALL_WORD_LIST_EXT };

	let words_ext_file = match File::open(words_ext_file_path) {
		Ok(f) => Some(f),
		_ => None
	};

	let mut words;

	if !options.save_extensions || words_ext_file.is_none() {
		println!("Building word extensions...");

		let words_file = File::open(words_file_path).expect(&format!("Failed to open words file {}.", words_file_path));

		println!("Reading words from {}...", words_file_path);
		words = read_words_from_file(words_file);
		println!("Read {} words.", words.len());

		println!("Finding extensions...");
		words = find_all_extensions(words, &options);
		println!("Found all extensions.");

		if options.save_extensions {
			println!("Writing words and extensions to {}", words_ext_file_path);
			let words_ext_file = File::create(words_ext_file_path).expect(&format!("Failed to open words file {}.", words_ext_file_path));
			write_words_and_extensions_to_file(words_ext_file, &words);
			println!("Finished writing words and extensions.");
		}
	} else {
		println!("Word extensions already built.");
		println!("Reading words and extensions from {}", words_ext_file_path);
		words = read_words_and_ext_from_file(words_ext_file.unwrap());
		println!("Read {} words (with extensions).", words.len());
	}

	let rand_word_start_idx = rand::random::<usize>() % words.len();
	let mut last_word_idx = rand_word_start_idx;
	let mut pbuf = String::new();
	let mut state = PortmantoutState::new(100000);

	if options.verbose {
		println!("added {}", &words[rand_word_start_idx].value);
		pbuf.push_str(&words[rand_word_start_idx].value);
	}

	'portmantout_loop: loop {
		if let Some(next_word_idx) = build_portmantout(last_word_idx, &mut words, &mut pbuf, &mut state, &options) {
			last_word_idx = next_word_idx;
		} else {
			break 'portmantout_loop
		}
	}

	println!("used {} words", state.chain);
	println!("portmantout is:");
	println!("{}", pbuf);
}