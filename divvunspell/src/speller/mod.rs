pub mod suggestion;
pub mod worker;

use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::f32;
use std::sync::Arc;

use self::worker::SpellerWorker;
use crate::speller::suggestion::Suggestion;
use crate::transducer::Transducer;
use crate::types::{SymbolNumber, Weight};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpellerConfig {
    pub n_best: Option<usize>,
    pub max_weight: Option<Weight>,
    pub beam: Option<Weight>,
    pub case_handling: bool,
    pub pool_start: usize,
    pub pool_max: usize,
    pub seen_node_sample_rate: u64,
}

impl SpellerConfig {
    pub fn default() -> SpellerConfig {
        SpellerConfig {
            n_best: None,
            max_weight: None,
            beam: None,
            case_handling: true,
            pool_start: 128,
            pool_max: 128,
            seen_node_sample_rate: 20,
        }
    }
}

#[derive(Debug)]
pub struct Speller<F, T: Transducer<F>, U: Transducer<F>>
where
    F: crate::vfs::File + crate::vfs::ToMemmap,
{
    mutator: T,
    lexicon: U,
    alphabet_translator: Vec<SymbolNumber>,
    _file: std::marker::PhantomData<F>,
}

impl<F, T, U> Speller<F, T, U>
where
    F: crate::vfs::File + crate::vfs::ToMemmap,
    T: Transducer<F>,
    U: Transducer<F>,
{
    pub fn new(mutator: T, mut lexicon: U) -> Arc<Speller<F, T, U>> {
        let alphabet_translator = lexicon.mut_alphabet().create_translator_from(&mutator);

        Arc::new(Speller {
            mutator,
            lexicon,
            alphabet_translator,
            _file: std::marker::PhantomData::<F>,
        })
    }

    pub fn mutator(&self) -> &T {
        &self.mutator
    }

    pub fn lexicon(&self) -> &U {
        &self.lexicon
    }

    fn alphabet_translator(&self) -> &Vec<SymbolNumber> {
        &self.alphabet_translator
    }

    fn to_input_vec(&self, word: &str) -> Vec<SymbolNumber> {
        let alphabet = self.mutator().alphabet();
        let key_table = alphabet.key_table();

        word.chars()
            .map(|ch| {
                let s = ch.to_string();
                key_table.iter().position(|x| x == &s)
                    .map(|x| x as u16)
                    .unwrap_or_else(|| alphabet.unknown().unwrap_or(0u16))
            })
            .collect()
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn is_correct(self: Arc<Self>, word: &str) -> bool {
        use crate::tokenizer::case_handling::*;

        let words = word_variants(self.lexicon().alphabet().key_table(), word);

        for word in words.into_iter() {
            let worker = SpellerWorker::new(
                self.clone(),
                self.to_input_vec(&word),
                SpellerConfig::default(),
            );

            if worker.is_correct() {
                return true;
            }
        }

        false
    }

    pub fn suggest(self: Arc<Self>, word: &str) -> Vec<Suggestion> {
        self.suggest_with_config(word, &SpellerConfig::default())
    }

    fn suggest_single(self: Arc<Self>, word: &str, config: &SpellerConfig) -> Vec<Suggestion> {
        let worker = SpellerWorker::new(self.clone(), self.to_input_vec(word), config.clone());

        worker.suggest()
    }

    fn suggest_caps_merging(
        self: Arc<Self>,
        ref_word: &str,
        words: Vec<SmolStr>,
        config: &SpellerConfig,
    ) -> Vec<Suggestion> {
        use crate::tokenizer::case_handling::*;

        let mut best: HashMap<SmolStr, f32> = HashMap::new();

        for word in words.into_iter() {
            let worker = SpellerWorker::new(self.clone(), self.to_input_vec(&word), config.clone());

            let suggestions = worker.suggest();

            if !suggestions.is_empty() {
                let r = if is_all_caps(ref_word) {
                    suggestions
                        .into_iter()
                        .map(|mut x| {
                            x.value = upper_case(x.value());
                            x
                        })
                        .collect()
                } else if is_first_caps(ref_word) {
                    suggestions
                        .into_iter()
                        .map(|mut x| {
                            x.value = upper_first(x.value());
                            x
                        })
                        .collect()
                } else {
                    suggestions
                };

                for sugg in r.into_iter() {
                    best.entry(sugg.value.clone())
                        .and_modify(|entry| {
                            if entry as &_ > &sugg.weight {
                                *entry = sugg.weight
                            }
                        })
                        .or_insert(sugg.weight);
                }
            }
        }

        let mut out = best
            .into_iter()
            .map(|(k, v)| Suggestion {
                value: k,
                weight: v,
            })
            .collect::<Vec<_>>();
        out.sort();
        if let Some(n_best) = config.n_best {
            out.truncate(n_best);
        }
        out
    }

    fn suggest_caps(
        self: Arc<Self>,
        ref_word: &str,
        words: Vec<SmolStr>,
        config: &SpellerConfig,
    ) -> Vec<Suggestion> {
        use crate::tokenizer::case_handling::*;

        for word in words.into_iter() {
            let worker = SpellerWorker::new(self.clone(), self.to_input_vec(&word), config.clone());

            let suggestions = worker.suggest();

            if !suggestions.is_empty() {
                if is_all_caps(ref_word) {
                    return suggestions
                        .into_iter()
                        .map(|mut x| {
                            x.value = upper_case(x.value());
                            x
                        })
                        .collect();
                } else if is_first_caps(ref_word) {
                    return suggestions
                        .into_iter()
                        .map(|mut x| {
                            x.value = upper_first(x.value());
                            x
                        })
                        .collect();
                }

                return suggestions;
            }
        }

        vec![]
    }

    pub fn suggest_with_config(
        self: Arc<Self>,
        word: &str,
        config: &SpellerConfig,
    ) -> Vec<Suggestion> {
        use crate::tokenizer::case_handling::*;

        if config.case_handling {
            let words = word_variants(self.lexicon().alphabet().key_table(), word);

            // TODO: check for the actual caps patterns, this is rather naive
            if words.len() == 2 || words.len() == 3 {
                self.suggest_caps_merging(word, words, config)
            } else {
                self.suggest_caps(word, words, config)
            }
        } else {
            self.suggest_single(word, config)
        }
    }
}
