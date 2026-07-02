#[cfg(test)]
mod tests {
    use super::super::HandwritingManager;
    use super::super::interfaces::TfHandwritingState;

    #[test]
    fn probe_set_handwriting_states() {
        let manager = HandwritingManager::new().expect("HandwritingManager::new");
        let current = manager.current_state().expect("current_state");
        eprintln!("current state: {:?}", current.0);

        for (name, state) in [
            ("AUTO", TfHandwritingState::AUTO),
            ("DISABLED", TfHandwritingState::DISABLED),
            ("ENABLED", TfHandwritingState(2)),
            ("POINTER_DELIVERY", TfHandwritingState::POINTER_DELIVERY),
        ] {
            let result = manager.set_state_for_test(state);
            eprintln!("SetHandwritingState({name}): {result:?}");
            let _ = manager.set_state_for_test(current);
        }
    }
}
