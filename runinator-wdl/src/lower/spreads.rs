// captures the authored `...alias` spread structure of a node's entry list as a self-contained
// "recipe" stored in graph metadata, so the decompiler can re-emit the spreads verbatim. the flat
// graph values stay the runtime source of truth; this sidecar is render-only.

use runinator_models::value::{Map, Value};

use crate::ast::*;
use crate::errors::WdlError;

use super::Lowerer;

impl Lowerer {
    // record the spread recipe for a node's primary entry list, keyed by node id. does nothing
    // when neither the list nor its nested values contain a spread, so spread-free nodes
    // decompile straight from the graph as before.
    pub(super) fn record_spreads(
        &mut self,
        node_id: &str,
        entries: &[(String, Expr)],
    ) -> Result<(), WdlError> {
        if !entries.iter().any(|(_, value)| expr_has_spread(value)) {
            return Ok(());
        }
        let segs = self.entry_segs(entries)?;
        self.spreads.insert(node_id.to_string(), Value::Array(segs));
        Ok(())
    }

    // turn an authored entry list into recipe segments: a `...alias` spread becomes
    // `{"spread": name}`; any other entry becomes `{"key": k, "value": <render value>}`.
    pub(super) fn entry_segs(&self, entries: &[(String, Expr)]) -> Result<Vec<Value>, WdlError> {
        let mut segs = Vec::with_capacity(entries.len());
        for (key, value) in entries {
            if let ExprKind::Spread(name) = &value.kind {
                segs.push(tagged("spread", Value::String(name.clone())));
                continue;
            }
            let mut seg = Map::new();
            seg.insert("key".into(), Value::String(key.clone()));
            seg.insert("value".into(), self.render_value(value)?);
            segs.push(Value::Object(seg));
        }
        Ok(segs)
    }

    // encode an authored value for the recipe. a spread-free value is stored as its lowered json
    // under `plain` (rendered by the decompiler's normal expr path); an object/array that carries
    // a spread keeps its structure under `object`/`array` so the spread survives the round trip.
    fn render_value(&self, expr: &Expr) -> Result<Value, WdlError> {
        match &expr.kind {
            ExprKind::Object(entries) if expr_has_spread(expr) => {
                Ok(tagged("object", Value::Array(self.entry_segs(entries)?)))
            }
            ExprKind::Array(items) if expr_has_spread(expr) => {
                let rendered = items
                    .iter()
                    .map(|item| self.render_value(item))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(tagged("array", Value::Array(rendered)))
            }
            _ => Ok(tagged("plain", self.lower_expr(expr)?)),
        }
    }
}

// whether an authored expression carries a `...alias` spread anywhere the recipe can represent it
// (directly, or nested inside object/array literals). spreads buried inside string interpolation
// or operator wrappers are treated as opaque and lowered flat (correct, but not resugared).
fn expr_has_spread(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Spread(_) => true,
        ExprKind::Object(entries) => entries.iter().any(|(_, value)| expr_has_spread(value)),
        ExprKind::Array(items) => items.iter().any(expr_has_spread),
        _ => false,
    }
}

// build a single-key object, the tagged-union shape both recipe segments and render values use.
fn tagged(key: &str, value: Value) -> Value {
    let mut map = Map::new();
    map.insert(key.to_string(), value);
    Value::Object(map)
}
