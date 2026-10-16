//! Dock y ventana en macOS (T10.2): la insignia con los pedidos pendientes y la decisión de traer
//! la ventana al frente o, si macOS no deja tomar el foco, hacer rebotar el Dock
//! (`request_user_attention(Critical)`) hasta que alguien la mire.

/// Texto de la insignia del Dock: nada con 0, el número hasta 99, «99+» después.
pub fn badge_text(count: u32) -> Option<String> {
    match count {
        0 => None,
        1..=99 => Some(count.to_string()),
        _ => Some("99+".into()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowState {
    pub visible: bool,
    pub minimized: bool,
    pub focused: bool,
    /// Otra app está en pantalla completa en el mismo espacio (macOS no deja quitarle el foco).
    pub other_app_fullscreen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attention {
    /// Ya está a la vista y con foco: no hacer nada.
    None,
    /// `unminimize` + `show` + `set_focus` y activar la app.
    BringToFront,
    /// Traer al frente y además hacer rebotar el Dock (macOS puede negar el foco).
    BringToFrontAndBounce,
    /// Sólo rebote del Dock y notificación (no robar el foco a otra app en pantalla completa).
    BounceOnly,
}

/// Qué hacer cuando entra un pedido nuevo.
pub fn attention_for(state: WindowState) -> Attention {
    if state.visible && state.focused && !state.minimized {
        return Attention::None;
    }
    if state.other_app_fullscreen {
        return Attention::BounceOnly;
    }
    if !state.visible || state.minimized {
        return Attention::BringToFrontAndBounce;
    }
    Attention::BringToFront
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insignia() {
        assert_eq!(badge_text(0), None);
        assert_eq!(badge_text(2).as_deref(), Some("2"));
        assert_eq!(badge_text(150).as_deref(), Some("99+"));
    }

    #[test]
    fn traer_al_frente_o_rebotar() {
        let base = WindowState { visible: true, minimized: false, focused: true, other_app_fullscreen: false };
        assert_eq!(attention_for(base), Attention::None);
        assert_eq!(attention_for(WindowState { focused: false, ..base }), Attention::BringToFront);
        assert_eq!(attention_for(WindowState { visible: false, focused: false, ..base }), Attention::BringToFrontAndBounce);
        assert_eq!(attention_for(WindowState { minimized: true, ..base }), Attention::BringToFrontAndBounce);
        assert_eq!(attention_for(WindowState { focused: false, other_app_fullscreen: true, ..base }), Attention::BounceOnly);
    }
}
