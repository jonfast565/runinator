use super::*;

#[test]
fn blocking_poll_invokes_the_supplied_function_until_complete() {
    let values = poll_pages(
        None,
        3,
        |cursor| match cursor {
            None => Ok::<_, &'static str>(PollPage {
                items: vec![1],
                next_cursor: Some("next"),
            }),
            Some("next") => Ok(PollPage {
                items: vec![2],
                next_cursor: None,
            }),
            _ => unreachable!(),
        },
        |_| "limit",
    )
    .unwrap();
    assert_eq!(values, [1, 2]);
}

#[tokio::test]
async fn async_poll_rejects_a_repeated_cursor() {
    let error = poll_pages_async(
        Some("same"),
        3,
        |_| async {
            Ok::<_, PollLimit>(PollPage::<(), _> {
                items: Vec::new(),
                next_cursor: Some("same"),
            })
        },
        |failure| failure,
    )
    .await
    .unwrap_err();
    assert_eq!(error, PollLimit::RepeatedCursor);
}

#[test]
fn blocking_poll_enforces_the_page_budget() {
    let error = poll_pages(
        None,
        1,
        |_| {
            Ok::<_, PollLimit>(PollPage::<(), _> {
                items: Vec::new(),
                next_cursor: Some(1),
            })
        },
        |failure| failure,
    )
    .unwrap_err();
    assert_eq!(error, PollLimit::PageBudget { max_pages: 1 });
}

#[test]
fn blocking_poll_rejects_a_cursor_cycle() {
    let error = poll_pages(
        Some("a"),
        4,
        |cursor| {
            Ok::<_, PollLimit>(PollPage {
                items: Vec::<u8>::new(),
                next_cursor: Some(if cursor == Some("a") { "b" } else { "a" }),
            })
        },
        |limit| limit,
    )
    .unwrap_err();
    assert_eq!(error, PollLimit::RepeatedCursor);
}

#[test]
fn a_page_number_walk_is_bounded_only_by_its_budget() {
    // the cursor guard compares cursor values, and a page number increments on every page, so a
    // source that ignores `page` and answers with the same body forever is never caught as a
    // cycle. it walks the whole budget and fails hard. this is the documented limit of the guard,
    // and the reason a caller paging by number has to check that its request honours the
    // parameter before it starts walking.
    let mut pages = 0;
    let error = poll_pages(
        Some(1u32),
        10,
        |page| {
            pages += 1;
            Ok::<_, PollLimit>(PollPage {
                items: vec![7u8; 100],
                next_cursor: Some(page.unwrap_or(1) + 1),
            })
        },
        |limit| limit,
    )
    .unwrap_err();
    assert_eq!(error, PollLimit::PageBudget { max_pages: 10 });
    assert_eq!(pages, 10, "every page in the budget was requested");
}
