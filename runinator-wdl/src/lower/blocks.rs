// expands wdl control blocks into the matching runinator control nodes. each builder
// receives its pre-allocated entry id and the continuation the block flows into.

use runinator_models::value::{Map, Value};

use crate::ast::*;
use crate::errors::WdlError;

use super::{Lowerer, node, node_ref};

impl Lowerer {
    pub(super) fn lower_if(
        &mut self,
        if_stmt: &IfStmt,
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
        self.push(node(
            id,
            "condition",
            vec![("transitions", Value::Object(transitions))],
        ));
        Ok(())
    }

    pub(super) fn lower_for(
        &mut self,
        for_stmt: &ForStmt,
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

        let mut transitions = Map::new();
        transitions.insert("next".into(), node_ref(&body_entry));
        transitions.insert("on_success".into(), node_ref(cont));

        let mut fields = vec![
            ("parameters", Value::Object(params)),
            ("transitions", Value::Object(transitions)),
        ];
        if let Some(limit) = for_stmt.limit {
            fields.push(("max_iterations", Value::from(limit)));
        }
        self.push(node(id, "loop", fields));
        Ok(())
    }

    pub(super) fn lower_map(
        &mut self,
        map_stmt: &MapStmt,
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

        self.push(node(
            id,
            "map",
            vec![
                ("parameters", Value::Object(params)),
                ("transitions", Value::Object(transitions)),
            ],
        ));
        Ok(())
    }

    pub(super) fn lower_match(
        &mut self,
        match_stmt: &MatchStmt,
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

        self.push(node(
            id,
            "switch",
            vec![("parameters", Value::Object(params))],
        ));
        Ok(())
    }

    pub(super) fn lower_parallel(
        &mut self,
        parallel: &ParallelStmt,
        id: &str,
        cont: &str,
    ) -> Result<(), WdlError> {
        let join_id = self.fresh("join");
        let mut branch_refs = Vec::new();
        for branch in &parallel.branches {
            let entry = self.lower_block(branch, &join_id)?;
            branch_refs.push(node_ref(&entry));
        }

        let mut parallel_params = Map::new();
        parallel_params.insert("branches".into(), Value::Array(branch_refs.clone()));
        self.push(node(
            id,
            "parallel",
            vec![("parameters", Value::Object(parallel_params))],
        ));

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
        self.push(node(
            id,
            "race",
            vec![
                ("parameters", Value::Object(params)),
                ("transitions", Value::Object(transitions)),
            ],
        ));
        Ok(())
    }

    pub(super) fn lower_try(
        &mut self,
        try_stmt: &TryStmt,
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
        self.push(node(
            id,
            "try",
            vec![
                ("parameters", Value::Object(params)),
                ("transitions", Value::Object(transitions)),
            ],
        ));
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
