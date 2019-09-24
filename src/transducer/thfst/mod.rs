#![allow(clippy::cast_ptr_alignment)] // FIXME: This at least needs a comment

use std::{u16, u32};

use crate::constants::TARGET_TABLE;
use crate::transducer::symbol_transition::SymbolTransition;
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
use serde_derive::{Deserialize, Serialize};

mod alphabet;
mod index_table;
mod transition_table;

use index_table::IndexTable;
use transition_table::TransitionTable;

use self::alphabet::TransducerAlphabet;
use crate::transducer::{Alphabet, Transducer};

#[repr(C)]
pub union WeightOrTarget {
    target: u32,
    weight: f32,
}

#[repr(C)]
pub struct IndexTableRecord {
    input_symbol: u16,
    #[doc(hidden)]
    __padding: u16,
    weight_or_target: WeightOrTarget,
}

#[repr(C)]
pub struct TransitionTableRecord {
    input_symbol: u16,
    output_symbol: u16,
    weight_or_target: WeightOrTarget,
}

#[derive(Serialize, Deserialize)]
pub struct MetaRecord {
    pub index_table_count: usize,
    pub transition_table_count: usize,
    pub chunk_size: usize,
    pub alphabet: TransducerAlphabet,
}

// impl MetaRecord {
//     pub fn serialize(&self, target_dir: &std::path::Path) {
//         use std::io::Write;

//         let s = serde_json::to_string_pretty(self).unwrap();
//         let mut f = std::fs::File::create(target_dir.join("meta")).unwrap();
//         writeln!(f, "{}", s).unwrap();
//     }
// }

/// Tromsø-Helsinki Finite State Transducer format
pub struct ThfstTransducer {
    // meta: MetaRecord,
    index_tables: Vec<IndexTable>,
    indexes_per_chunk: u32,
    transition_tables: Vec<TransitionTable>,
    transitions_per_chunk: u32,
    alphabet: TransducerAlphabet,
}

impl ThfstTransducer {
    // pub fn from_path(path: &std::path::Path) -> Result<Self, std::io::Error> {
    //     // Load meta
    //     let meta_file = File::open(path.join("meta")).map_err(|_| {
    //         std::io::Error::new(
    //             std::io::ErrorKind::NotFound,
    //             format!(
    //                 "`meta` not found in transducer path, looked for {}",
    //                 path.join("meta").display()
    //             ),
    //         )
    //     })?;
    //     let meta: MetaRecord = serde_json::from_reader(meta_file)?;

    //     let mut index_tables = vec![];
    //     for i in 0..meta.index_table_count {
    //         let filename = format!("index-{:02}", i);
    //         let fpath = path.join(&filename);
    //         let index_table = IndexTable::from_path(&fpath).map_err(|_| {
    //             std::io::Error::new(
    //                 std::io::ErrorKind::NotFound,
    //                 &*format!("{} not found in transducer path", &filename),
    //             )
    //         })?;
    //         index_tables.push(index_table);
    //     }

    //     let indexes_per_chunk = meta.chunk_size as u32 / 8u32;

    //     let mut transition_tables = vec![];
    //     for i in 0..meta.transition_table_count {
    //         let filename = format!("transition-{:02}", i);
    //         let fpath = path.join(&filename);
    //         let transition_table = TransitionTable::from_path(&fpath).map_err(|_| {
    //             std::io::Error::new(
    //                 std::io::ErrorKind::NotFound,
    //                 &*format!("{} not found in transducer path", &filename),
    //             )
    //         })?;
    //         transition_tables.push(transition_table);
    //     }

    //     let transitions_per_chunk = meta.chunk_size as u32 / 12u32;

    //     let alphabet = TransducerAlphabetParser::parse(&meta.raw_alphabet);

    //     Ok(ThfstTransducer {
    //         // meta,
    //         index_tables,
    //         indexes_per_chunk,
    //         transition_tables,
    //         transitions_per_chunk,
    //         alphabet,
    //     })
    // }

    #[inline]
    fn transition_rel_index(&self, x: TransitionTableIndex) -> (usize, TransitionTableIndex) {
        let index_page = x / self.transitions_per_chunk;
        let relative_index = x - (self.transitions_per_chunk * index_page);
        (index_page as usize, relative_index)
    }

    #[inline]
    fn index_rel_index(&self, x: TransitionTableIndex) -> (usize, TransitionTableIndex) {
        let index_page = x / self.indexes_per_chunk;
        let relative_index = x - (self.indexes_per_chunk * index_page);
        (index_page as usize, relative_index)
    }
}

impl Transducer for ThfstTransducer {
    type Alphabet = TransducerAlphabet;

    #[inline(always)]
    fn alphabet(&self) -> &TransducerAlphabet {
        &self.alphabet
    }

    #[inline(always)]
    fn mut_alphabet(&mut self) -> &mut TransducerAlphabet {
        &mut self.alphabet
    }

    #[inline(always)]
    fn transition_input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        let (page, index) = self.transition_rel_index(i);
        self.transition_tables[page].input_symbol(index)
    }

    #[inline(always)]
    fn is_final(&self, i: TransitionTableIndex) -> bool {
        if i >= TARGET_TABLE {
            let (page, index) = self.transition_rel_index(i - TARGET_TABLE);
            self.transition_tables[page].is_final(index)
        } else {
            let (page, index) = self.index_rel_index(i);
            self.index_tables[page].is_final(index)
        }
    }

    #[inline(always)]
    fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        if i >= TARGET_TABLE {
            let (page, index) = self.transition_rel_index(i - TARGET_TABLE);
            self.transition_tables[page].weight(index)
        } else {
            let (page, index) = self.index_rel_index(i);
            self.index_tables[page].final_weight(index)
        }
    }

    #[inline(always)]
    fn has_transitions(&self, i: TransitionTableIndex, s: Option<SymbolNumber>) -> bool {
        let sym = match s {
            Some(v) => v,
            None => return false,
        };

        if i >= TARGET_TABLE {
            let (page, index) = self.transition_rel_index(i - TARGET_TABLE);
            match self.transition_tables[page].input_symbol(index) {
                Some(res) => sym == res,
                None => false,
            }
        } else {
            let (page, index) = self.index_rel_index(i + u32::from(sym));
            match self.index_tables[page].input_symbol(index) {
                Some(res) => sym == res,
                None => false,
            }
        }
    }

    #[inline(always)]
    fn has_epsilons_or_flags(&self, i: TransitionTableIndex) -> bool {
        if i >= TARGET_TABLE {
            let (page, index) = self.transition_rel_index(i - TARGET_TABLE);
            match self.transition_tables[page].input_symbol(index) {
                Some(sym) => sym == 0 || self.alphabet.is_flag(sym),
                None => false,
            }
        } else {
            let (page, index) = self.index_rel_index(i);
            if let Some(0) = self.index_tables[page].input_symbol(index) {
                true
            } else {
                false
            }
        }
    }

    #[inline(always)]
    fn take_epsilons(&self, i: TransitionTableIndex) -> Option<SymbolTransition> {
        let (page, index) = self.transition_rel_index(i);

        if let Some(0) = self.transition_tables[page].input_symbol(index) {
            Some(self.transition_tables[page].symbol_transition(index))
        } else {
            None
        }
    }

    #[inline(always)]
    fn take_epsilons_and_flags(&self, i: TransitionTableIndex) -> Option<SymbolTransition> {
        let (page, index) = self.transition_rel_index(i);

        if let Some(sym) = self.transition_tables[page].input_symbol(index) {
            if sym != 0 && !self.alphabet.is_flag(sym) {
                None
            } else {
                Some(self.transition_tables[page].symbol_transition(index))
            }
        } else {
            None
        }
    }

    #[inline(always)]
    fn take_non_epsilons(
        &self,
        i: TransitionTableIndex,
        symbol: SymbolNumber,
    ) -> Option<SymbolTransition> {
        let (page, index) = self.transition_rel_index(i);
        if let Some(input_sym) = self.transition_tables[page].input_symbol(index) {
            if input_sym != symbol {
                None
            } else {
                Some(self.transition_tables[page].symbol_transition(index))
            }
        } else {
            None
        }
    }

    #[inline(always)]
    fn next(&self, i: TransitionTableIndex, symbol: SymbolNumber) -> Option<TransitionTableIndex> {
        if i >= TARGET_TABLE {
            Some(i - TARGET_TABLE + 1)
        } else {
            let (page, index) = self.index_rel_index(i + 1 + u32::from(symbol));

            if let Some(v) = self.index_tables[page].target(index) {
                Some(v - TARGET_TABLE)
            } else {
                None
            }
        }
    }
}

// pub struct ThfstBundle {
//     pub lexicon: ThfstTransducer,
//     pub mutator: ThfstTransducer,
// }

// impl ThfstBundle {
//     pub fn from_path(path: &std::path::Path) -> Result<Self, std::io::Error> {
//         let lexicon = ThfstTransducer::from_path(&path.join("lexicon"))?;
//         let mutator = ThfstTransducer::from_path(&path.join("mutator"))?;

//         Ok(ThfstBundle { lexicon, mutator })
//     }

//     pub fn speller(self) -> Arc<Speller<ThfstTransducer>> {
//         Speller::new(self.mutator, self.lexicon)
//     }
// }
