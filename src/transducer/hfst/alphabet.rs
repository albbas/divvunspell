use super::Transducer;
use crate::types::{FlagDiacriticOperation, FlagDiacriticOperator, SymbolNumber, ValueNumber, OperationsMap};
use hashbrown::HashMap;
use smol_str::SmolStr;

use super::super::Alphabet;

#[derive(Debug)]
pub struct TransducerAlphabet {
    pub(crate) key_table: Vec<SmolStr>,
    pub(crate) initial_symbol_count: SymbolNumber,
    pub(crate) flag_state_size: SymbolNumber,
    pub(crate) length: usize,
    pub(crate) string_to_symbol: HashMap<SmolStr, SymbolNumber>,
    pub(crate) operations: OperationsMap,
    pub(crate) identity_symbol: Option<SymbolNumber>,
    pub(crate) unknown_symbol: Option<SymbolNumber>,
}

struct TransducerAlphabetParser {
    key_table: Vec<SmolStr>,
    flag_state_size: SymbolNumber,
    length: usize,
    string_to_symbol: HashMap<SmolStr, SymbolNumber>,
    operations: OperationsMap,
    feature_bucket: HashMap<SmolStr, SymbolNumber>,
    value_bucket: HashMap<SmolStr, ValueNumber>,
    val_n: ValueNumber,
    feat_n: SymbolNumber,
    identity_symbol: Option<SymbolNumber>,
    unknown_symbol: Option<SymbolNumber>,
}

impl TransducerAlphabetParser {
    fn new() -> TransducerAlphabetParser {
        TransducerAlphabetParser {
            key_table: Vec::with_capacity(64),
            flag_state_size: 0,
            length: 0,
            string_to_symbol: HashMap::new(),
            operations: HashMap::new(),
            feature_bucket: HashMap::new(),
            value_bucket: HashMap::new(),
            val_n: 0i16,
            feat_n: 0u16,
            identity_symbol: None,
            unknown_symbol: None,
        }
    }

    fn handle_special_symbol(&mut self, i: SymbolNumber, key: &str) {
        let mut chunks = key.split('.');

        let fdo = FlagDiacriticOperator::from_str(&chunks.next().unwrap()[1..]).unwrap();
        let feature: SmolStr = chunks
            .next()
            .unwrap_or("")
            .chars()
            .filter(|x| x != &'@')
            .collect();
        let value: SmolStr = chunks
            .next()
            .unwrap_or("")
            .chars()
            .filter(|x| x != &'@')
            .collect();

        if !self.feature_bucket.contains_key(&feature) {
            self.feature_bucket.insert(feature.clone(), self.feat_n);
            self.feat_n += 1;
        }

        if !self.value_bucket.contains_key(&value) {
            self.value_bucket.insert(value.clone(), self.val_n);
            self.val_n += 1;
        }

        let op = FlagDiacriticOperation {
            operation: fdo,
            feature: self.feature_bucket[&feature],
            value: self.value_bucket[&value],
        };

        self.operations.insert(i, op);
        self.key_table.push(key.into());
    }

    fn parse_inner(&mut self, buf: &[u8], symbols: SymbolNumber) {
        let mut offset = 0usize;

        for i in 0..symbols {
            let mut end = 0usize;

            while buf[offset + end] != 0 {
                end += 1;
            }

            let key: SmolStr = String::from_utf8_lossy(&buf[offset..offset + end]).into();

            if key.len() > 1 && key.starts_with('@') && key.ends_with('@') {
                if key.chars().nth(2).unwrap() == '.' {
                    self.handle_special_symbol(i, &key);
                } else if key == "@_EPSILON_SYMBOL_@" {
                    self.value_bucket.insert("".into(), self.val_n);
                    self.key_table.push("".into());
                    self.val_n += 1;
                } else if key == "@_IDENTITY_SYMBOL_@" {
                    self.identity_symbol = Some(i);
                    self.key_table.push(key);
                } else if key == "@_UNKNOWN_SYMBOL_@" {
                    self.unknown_symbol = Some(i);
                    self.key_table.push(key);
                } else {
                    // No idea, skip.
                    eprintln!("Unhandled alphabet key: {}", &key);
                    self.key_table.push(SmolStr::from(""));
                }
            } else {
                self.key_table.push(key.clone());
                self.string_to_symbol.insert(key.clone(), i);
            }

            offset += end + 1;
        }

        self.flag_state_size = self.feature_bucket.len() as SymbolNumber;

        // Count remaining null padding bytes
        while buf[offset] == b'\0' {
            offset += 1;
        }

        self.length = offset;
    }

    fn parse(buf: &[u8], symbols: SymbolNumber) -> TransducerAlphabet {
        let mut p = TransducerAlphabetParser::new();
        p.parse_inner(buf, symbols);

        TransducerAlphabet {
            key_table: p.key_table,
            initial_symbol_count: symbols,
            length: p.length,
            flag_state_size: p.flag_state_size,
            string_to_symbol: p.string_to_symbol,
            operations: p.operations,
            identity_symbol: p.identity_symbol,
            unknown_symbol: p.unknown_symbol,
        }
    }
}

impl TransducerAlphabet {
    pub fn new(buf: &[u8], symbols: SymbolNumber) -> TransducerAlphabet {
        TransducerAlphabetParser::parse(buf, symbols)
    }
}

impl Alphabet for TransducerAlphabet {
    fn key_table(&self) -> &Vec<SmolStr> {
        &self.key_table
    }

    fn state_size(&self) -> SymbolNumber {
        self.flag_state_size
    }

    fn operations(&self) -> &OperationsMap {
        &self.operations
    }

    fn string_to_symbol(&self) -> &HashMap<SmolStr, SymbolNumber> {
        &self.string_to_symbol
    }

    fn is_flag(&self, symbol: SymbolNumber) -> bool {
        self.operations.contains_key(&symbol)
    }

    fn add_symbol(&mut self, string: &str) {
        self.string_to_symbol
            .insert(string.into(), self.key_table.len() as u16);
        self.key_table.push(string.into());
    }

    fn identity(&self) -> Option<SymbolNumber> {
        self.identity_symbol
    }

    fn unknown(&self) -> Option<SymbolNumber> {
        self.unknown_symbol
    }

    fn initial_symbol_count(&self) -> SymbolNumber {
        self.initial_symbol_count
    }

    fn len(&self) -> usize {
        self.length
    }

    fn is_empty(&self) -> bool {
        self.length == 0
    }

    fn create_translator_from(&mut self, mutator: &dyn Transducer<Alphabet = Self>) -> Vec<SymbolNumber> {
        let from = mutator.alphabet();
        let from_keys = from.key_table();

        let mut translator = Vec::with_capacity(64);
        translator.push(0);

        for from_sym in from_keys.iter().skip(1) {
            if let Some(&sym) = self.string_to_symbol.get(from_sym) {
                translator.push(sym);
            } else {
                let lexicon_key = self.key_table.len() as SymbolNumber;
                translator.push(lexicon_key);
                self.add_symbol(from_sym);
            }
        }

        translator
    }
}
