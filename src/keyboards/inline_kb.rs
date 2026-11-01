use crate::services::safe_send::{btn_url, KbButton};
use crate::texts::messages::{
    EMOJI_CONFIRM_BTN, EMOJI_CREATE_BTN, EMOJI_LINK_BTN, EMOJI_REJECT_BTN, EMOJI_RULES_BTN,
};

pub fn rules_keyboard() -> Vec<Vec<KbButton>> {
    vec![vec![btn_url(
        "Правила проведения сделки",
        Some(EMOJI_RULES_BTN),
        &crate::config::config().rules_url,
    )
    .with_style("primary")]]
}

pub fn deal_go_keyboard(group_link: &str) -> Vec<Vec<KbButton>> {
    vec![vec![btn_url("Перейти в сделку", Some(EMOJI_LINK_BTN), group_link).with_style("primary")]]
}

pub fn ruchenik_keyboard(deal_id: i32, target_user_id: i64) -> Vec<Vec<KbButton>> {
    vec![vec![
        KbButton::callback(
            "Подтвердить",
            Some(EMOJI_CONFIRM_BTN),
            &format!("ruchenik:confirm:{}:{}", deal_id, target_user_id),
        )
        .with_style("success"),
        KbButton::callback(
            "Отклонить",
            Some(EMOJI_REJECT_BTN),
            &format!("ruchenik:cancel:{}", deal_id),
        )
        .with_style("danger"),
    ]]
}

pub fn cancel_deal_keyboard(group_id: i64) -> Vec<Vec<KbButton>> {
    vec![vec![
        KbButton::callback(
            "Подтвердить",
            Some(EMOJI_CONFIRM_BTN),
            &format!("deal_cancel:yes:{}", group_id),
        )
        .with_style("success"),
        KbButton::callback(
            "Отклонить",
            Some(EMOJI_REJECT_BTN),
            &format!("deal_cancel:no:{}", group_id),
        )
        .with_style("danger"),
    ]]
}

pub fn inline_create_button(deal_display: &str, callback_data: &str) -> Vec<Vec<KbButton>> {
    vec![vec![KbButton::callback(
        &format!("Создать группу: {}", deal_display),
        Some(EMOJI_CREATE_BTN),
        callback_data,
    )
    .with_style("primary")]]
}

/// Кнопка подтверждения сброса всех сделок (inline «reset all»).
pub fn reset_all_confirm_button(creator_id: i64) -> Vec<Vec<KbButton>> {
    vec![vec![KbButton::callback(
        "Сбросить все сделки",
        Some(crate::texts::messages::EMOJI_RESET),
        &format!("resetall:confirm:{}", creator_id),
    )
    .with_style("danger")]]
}
