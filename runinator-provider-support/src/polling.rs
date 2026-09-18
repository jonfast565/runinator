use std::future::Future;

/// One page returned by an externally supplied polling function.
pub struct PollPage<T, C> {
    pub items: Vec<T>,
    pub next_cursor: Option<C>,
}

/// Why a bounded poll could not safely reach its end.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PollLimit {
    RepeatedCursor,
    PageBudget { max_pages: usize },
}

/// Repeatedly invoke a blocking page function until it reports no next cursor.
pub fn poll_pages<T, C, E>(
    initial_cursor: Option<C>,
    max_pages: usize,
    mut fetch: impl FnMut(Option<C>) -> Result<PollPage<T, C>, E>,
    failure: impl Fn(PollLimit) -> E,
) -> Result<Vec<T>, E>
where
    C: Clone + Eq,
{
    if max_pages == 0 {
        return Err(failure(PollLimit::PageBudget { max_pages }));
    }
    let mut cursor = initial_cursor;
    let mut seen_cursors = cursor.iter().cloned().collect::<Vec<_>>();
    let mut items = Vec::new();
    for page_number in 0..max_pages {
        let page = fetch(cursor.clone())?;
        items.extend(page.items);
        let Some(next_cursor) = page.next_cursor else {
            return Ok(items);
        };
        if seen_cursors.contains(&next_cursor) {
            return Err(failure(PollLimit::RepeatedCursor));
        }
        if page_number + 1 == max_pages {
            return Err(failure(PollLimit::PageBudget { max_pages }));
        }
        seen_cursors.push(next_cursor.clone());
        cursor = Some(next_cursor);
    }
    Ok(items)
}

/// Repeatedly invoke an async page function until it reports no next cursor.
pub async fn poll_pages_async<T, C, E, F, Fut>(
    initial_cursor: Option<C>,
    max_pages: usize,
    mut fetch: F,
    failure: impl Fn(PollLimit) -> E,
) -> Result<Vec<T>, E>
where
    C: Clone + Eq,
    F: FnMut(Option<C>) -> Fut,
    Fut: Future<Output = Result<PollPage<T, C>, E>>,
{
    if max_pages == 0 {
        return Err(failure(PollLimit::PageBudget { max_pages }));
    }
    let mut cursor = initial_cursor;
    let mut seen_cursors = cursor.iter().cloned().collect::<Vec<_>>();
    let mut items = Vec::new();
    for page_number in 0..max_pages {
        let page = fetch(cursor.clone()).await?;
        items.extend(page.items);
        let Some(next_cursor) = page.next_cursor else {
            return Ok(items);
        };
        if seen_cursors.contains(&next_cursor) {
            return Err(failure(PollLimit::RepeatedCursor));
        }
        if page_number + 1 == max_pages {
            return Err(failure(PollLimit::PageBudget { max_pages }));
        }
        seen_cursors.push(next_cursor.clone());
        cursor = Some(next_cursor);
    }
    Ok(items)
}

#[cfg(test)]
#[path = "polling_tests.rs"]
mod tests;
