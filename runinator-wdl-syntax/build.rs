use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::env;
use std::fs;
use std::path::PathBuf;

use pest_meta::ast::{Expr, Rule};
use pest_meta::parser;

fn main() {
    println!("cargo:rerun-if-changed=src/wdl.pest");

    let grammar = fs::read_to_string("src/wdl.pest").expect("read wdl grammar");
    let pairs = parser::parse(parser::Rule::grammar_rules, &grammar).expect("parse wdl grammar");
    pest_meta::validator::validate_pairs(pairs.clone()).expect("validate wdl grammar");
    let rules = parser::consume_rules(pairs).expect("consume wdl grammar");
    let by_name = rules
        .iter()
        .map(|rule| (rule.name.as_str(), rule))
        .collect::<BTreeMap<_, _>>();

    let grammar_keywords = grammar_keywords(&rules);
    let statement_keywords = statement_keywords(&by_name);
    let generated = format!(
        "pub const GRAMMAR_KEYWORDS: &[&str] = &{};\n\
         pub const STATEMENT_KEYWORDS: &[&str] = &{};\n",
        string_slice(&grammar_keywords),
        string_slice(&statement_keywords),
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    fs::write(out.join("grammar_vocabulary.rs"), generated).expect("write grammar vocabulary");
}

fn grammar_keywords(rules: &[Rule]) -> BTreeSet<String> {
    let mut keywords = BTreeSet::new();
    for rule in rules {
        for expr in rule.expr.iter_top_down() {
            if let Expr::Str(value) | Expr::Insens(value) = expr
                && is_word(&value)
            {
                keywords.insert(value.clone());
            }
        }
    }
    keywords
}

fn statement_keywords(rules: &BTreeMap<&str, &Rule>) -> BTreeSet<String> {
    let mut rule_names = BTreeSet::new();
    for root in ["document_item", "stmt_body"] {
        let rule = rules
            .get(root)
            .unwrap_or_else(|| panic!("missing {root} rule"));
        collect_choice_rules(&rule.expr, &mut rule_names);
    }
    // `node` is an optional binding prefix around a statement body, not a body rule itself.
    rule_names.insert("node_kw".to_string());

    let mut keywords = BTreeSet::new();
    for name in rule_names {
        if name == "stmt" {
            continue;
        }
        let mut visiting = HashSet::new();
        collect_leading_words(&Expr::Ident(name), rules, &mut visiting, &mut keywords);
    }
    keywords
}

fn collect_choice_rules(expr: &Expr, names: &mut BTreeSet<String>) {
    match expr {
        Expr::Choice(left, right) => {
            collect_choice_rules(left, names);
            collect_choice_rules(right, names);
        }
        Expr::Ident(name) => {
            names.insert(name.clone());
        }
        _ => {}
    }
}

fn collect_leading_words(
    expr: &Expr,
    rules: &BTreeMap<&str, &Rule>,
    visiting: &mut HashSet<String>,
    words: &mut BTreeSet<String>,
) {
    match expr {
        Expr::Str(value) | Expr::Insens(value) if is_word(value) => {
            words.insert(value.clone());
        }
        Expr::Ident(name) if visiting.insert(name.clone()) => {
            if let Some(rule) = rules.get(name.as_str()) {
                collect_leading_words(&rule.expr, rules, visiting, words);
            }
            visiting.remove(name);
        }
        Expr::Choice(left, right) => {
            collect_leading_words(left, rules, visiting, words);
            collect_leading_words(right, rules, visiting, words);
        }
        Expr::Seq(left, _) => collect_leading_words(left, rules, visiting, words),
        Expr::PosPred(inner)
        | Expr::NegPred(inner)
        | Expr::Opt(inner)
        | Expr::Rep(inner)
        | Expr::RepOnce(inner)
        | Expr::RepExact(inner, _)
        | Expr::RepMin(inner, _)
        | Expr::RepMax(inner, _)
        | Expr::RepMinMax(inner, _, _)
        | Expr::Push(inner) => collect_leading_words(inner, rules, visiting, words),
        _ => {}
    }
}

fn is_word(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn string_slice(values: &BTreeSet<String>) -> String {
    let values = values
        .iter()
        .map(|value| format!("{value:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{values}]")
}
