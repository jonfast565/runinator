// expands wdl control blocks into the matching runinator control nodes. each builder
// receives its pre-allocated entry id and the continuation the block flows into.

use runinator_models::value::{Map, Value};

use crate::ast::*;
use crate::errors::WdlError;

use super::{Lowerer, node, node_ref};

// safety cap applied to a `while`/`until` loop that omits an explicit `limit`, so a
// condition that never settles cannot loop forever. surfaced in decompiled source.
const DEFAULT_WHILE_LIMIT: i64 = 1000;

impl Lowerer {
    pub(super) fn lower_if(
        &mut self,
        if_stmt: &IfStmt,
        stmt: &Stmt,
        id: &str,
        cont: &str,
    ) -> Result<(), WdlError> {
        let mut branches = Vec::new();
        for (cond, body) in &if_stmt.arms {
            let entry = self.lower_block(body, cont)?;
            let mut branch = Map::new();
            branch.insert("when".into(), self.lower_cond(cond)?);
            branch.insert("target".into(), node_ref(&entry));
            branches.push(Value::Object(branch));
        }
        let else_entry = match &if_stmt.else_block {
            Some(block) => self.lower_block(block, cont)?,
            None => cont.to_string(),
        };
        let mut transitions = Map::new();
        transitions.insert("branches".into(), Value::Array(branches));
        transitions.insert("next".into(), node_ref(&else_entry));
        let mut fields = vec![("transitions", Value::Object(transitions))];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "condition", fields));
        Ok(())
    }

    pub(super) fn lower_for(
        &mut self,
        for_stmt: &ForStmt,
        stmt: &Stmt,
        id: &str,
        cont: &str,
    ) -> Result<(), WdlError> {
        let items = self.lower_expr(&for_stmt.items)?;

        // the loop body iterates and returns to the loop node; on exhaustion it exits.
        self.push_scope(&for_stmt.var, id, vec![PathSeg::Key("item".into())]);
        let body_entry = self.lower_block(&for_stmt.body, id)?;
        self.pop_scope();

        let mut params = Map::new();
        params.insert("items".into(), items);

        // a literal cap becomes the typed `max_iterations` field; any other expression
        // is carried in the loop parameters and resolved against the run context.
        let mut literal_limit = None;
        if let Some(limit) = &for_stmt.limit {
            let lowered = self.lower_expr(limit)?;
            match lowered.as_i64() {
                Some(n) => literal_limit = Some(n),
                None => {
                    params.insert("max_iterations".into(), lowered);
                }
            }
        }

        let mut transitions = Map::new();
        transitions.insert("next".into(), node_ref(&body_entry));
        transitions.insert("on_success".into(), node_ref(cont));

        let mut fields = vec![
            ("parameters", Value::Object(params)),
            ("transitions", Value::Object(transitions)),
        ];
        if let Some(limit) = literal_limit {
            fields.push(("max_iterations", Value::from(limit)));
        }
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "loop", fields));
        Ok(())
    }

    pub(super) fn lower_while(
        &mut self,
        while_stmt: &WhileStmt,
        stmt: &Stmt,
        id: &str,
        cont: &str,
    ) -> Result<(), WdlError> {
        // the loop condition; `until` negates it so the loop runs while `!cond`.
        let mut condition = self.lower_cond(&while_stmt.cond)?;
        if while_stmt.negate {
            let mut not_map = Map::new();
            not_map.insert("not".into(), condition);
            condition = Value::Object(not_map);
        }

        // the body loops back to this header; a false condition falls through to cont.
        let body_entry = self.lower_block(&while_stmt.body, id)?;

        let mut branch = Map::new();
        branch.insert("when".into(), condition);
        branch.insert("target".into(), node_ref(&body_entry));

        let mut transitions = Map::new();
        transitions.insert("branches".into(), Value::Array(vec![Value::Object(branch)]));
        transitions.insert("next".into(), node_ref(cont));

        // reentry both authorizes the back-edge in validation and bounds the loop; hitting
        // the cap exits to cont, matching the false-condition exit.
        let max_visits = while_stmt.limit.unwrap_or(DEFAULT_WHILE_LIMIT);
        let mut reentry = Map::new();
        reentry.insert("enabled".into(), Value::Bool(true));
        reentry.insert("max_visits".into(), Value::from(max_visits));
        reentry.insert("on_exhausted".into(), node_ref(cont));

        let mut fields = vec![
            ("transitions", Value::Object(transitions)),
            ("reentry", Value::Object(reentry)),
        ];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "condition", fields));
        Ok(())
    }

    pub(super) fn lower_map(
        &mut self,
        map_stmt: &MapStmt,
        stmt: &Stmt,
        id: &str,
        cont: &str,
    ) -> Result<(), WdlError> {
        let items = self.lower_expr(&map_stmt.items)?;

        self.push_scope(&map_stmt.var, id, vec![PathSeg::Key("item".into())]);
        let body_entry = self.lower_block(&map_stmt.body, id)?;
        self.pop_scope();

        let mut params = Map::new();
        params.insert("items".into(), items);
        params.insert("target".into(), node_ref(&body_entry));
        if let Some(concurrency) = map_stmt.concurrency {
            params.insert("concurrency".into(), Value::from(concurrency));
        }

        let mut transitions = Map::new();
        transitions.insert("next".into(), node_ref(cont));

        let mut fields = vec![
            ("parameters", Value::Object(params)),
            ("transitions", Value::Object(transitions)),
        ];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "map", fields));
        Ok(())
    }

    pub(super) fn lower_match(
        &mut self,
        match_stmt: &MatchStmt,
        stmt: &Stmt,
        id: &str,
        cont: &str,
    ) -> Result<(), WdlError> {
        let value = self.lower_expr(&match_stmt.subject)?;
        let mut cases = Vec::new();
        for arm in &match_stmt.arms {
            let entry = self.lower_block(&arm.body, cont)?;
            let mut case = Map::new();
            if let Some(when) = &arm.when {
                case.insert("when".into(), self.lower_cond(when)?);
            } else if let Some(equals) = &arm.equals {
                case.insert("equals".into(), self.lower_expr(equals)?);
            } else {
                return Err(WdlError::lower("match arm needs a value or when clause"));
            }
            case.insert("target".into(), node_ref(&entry));
            cases.push(Value::Object(case));
        }
        let default_entry = match &match_stmt.default {
            Some(block) => self.lower_block(block, cont)?,
            None => cont.to_string(),
        };

        let mut params = Map::new();
        params.insert("value".into(), value);
        params.insert("cases".into(), Value::Array(cases));
        params.insert("default".into(), node_ref(&default_entry));

        let mut fields = vec![("parameters", Value::Object(params))];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "switch", fields));
        Ok(())
    }

    pub(super) fn lower_parallel(
        &mut self,
        parallel: &ParallelStmt,
        stmt: &Stmt,
        id: &str,
        cont: &str,
    ) -> Result<(), WdlError> {
        // derive the join id from the parallel's id so it stays stable across a round trip: the
        // join has no surface form, so a counter-based id would drift whenever explicit ids shift
        // the counter. falls back to a fresh id only on the unlikely collision.
        let join_id = self
            .claim(&format!("{id}_join"))
            .unwrap_or_else(|_| self.fresh("join"));
        let mut branch_refs = Vec::new();
        for branch in &parallel.branches {
            let entry = self.lower_block(branch, &join_id)?;
            branch_refs.push(node_ref(&entry));
        }

        let mut parallel_params = Map::new();
        parallel_params.insert("branches".into(), Value::Array(branch_refs.clone()));
        let mut parallel_fields = vec![("parameters", Value::Object(parallel_params))];
        self.apply_annotations(&mut parallel_fields, stmt);
        self.push(node(id, "parallel", parallel_fields));

        let mut join_params = Map::new();
        join_params.insert("wait_for".into(), Value::Array(branch_refs));
        join_params.insert(
            "mode".into(),
            Value::String(policy_str(parallel.join).into()),
        );
        let mut join_transitions = Map::new();
        join_transitions.insert("next".into(), node_ref(cont));
        self.push(node(
            &join_id,
            "join",
            vec![
                ("parameters", Value::Object(join_params)),
                ("transitions", Value::Object(join_transitions)),
            ],
        ));
        Ok(())
    }

    pub(super) fn lower_race(
        &mut self,
        race: &RaceStmt,
        stmt: &Stmt,
        id: &str,
        cont: &str,
    ) -> Result<(), WdlError> {
        let mut branch_refs = Vec::new();
        for branch in &race.branches {
            let entry = self.lower_block(branch, cont)?;
            branch_refs.push(node_ref(&entry));
        }
        let mut params = Map::new();
        params.insert("branches".into(), Value::Array(branch_refs));
        params.insert(
            "winner".into(),
            Value::String(policy_str(race.winner).into()),
        );
        let mut transitions = Map::new();
        transitions.insert("next".into(), node_ref(cont));
        let mut fields = vec![
            ("parameters", Value::Object(params)),
            ("transitions", Value::Object(transitions)),
        ];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "race", fields));
        Ok(())
    }

    pub(super) fn lower_try(
        &mut self,
        try_stmt: &TryStmt,
        stmt: &Stmt,
        id: &str,
        cont: &str,
    ) -> Result<(), WdlError> {
        let body_entry = self.lower_block(&try_stmt.body, cont)?;
        let catch_entry = match &try_stmt.catch {
            Some(block) => Some(self.lower_block(block, cont)?),
            None => None,
        };
        let finally_entry = match &try_stmt.finally {
            Some(block) => Some(self.lower_block(block, cont)?),
            None => None,
        };

        let mut params = Map::new();
        params.insert("body".into(), node_ref(&body_entry));
        if let Some(entry) = &catch_entry {
            params.insert("catch".into(), node_ref(entry));
        }
        if let Some(entry) = &finally_entry {
            params.insert("finally".into(), node_ref(entry));
        }
        let mut transitions = Map::new();
        transitions.insert("next".into(), node_ref(cont));
        let mut fields = vec![
            ("parameters", Value::Object(params)),
            ("transitions", Value::Object(transitions)),
        ];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "try", fields));
        Ok(())
    }
}

fn policy_str(policy: BranchPolicy) -> &'static str {
    match policy {
        BranchPolicy::All => "all",
        BranchPolicy::Any => "any",
        BranchPolicy::FirstSuccess => "first_success",
    }
}
