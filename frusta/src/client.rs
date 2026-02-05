use uuid::Uuid;

pub struct ViewportContext {
    pub id: Uuid,
    pub x: f32,
    pub y: f32,
    pub width: u32,
    pub height: u32,
    pub zoom: f32,
}
