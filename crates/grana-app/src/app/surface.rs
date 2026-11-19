use std::sync::Arc;

use grana_display::SurfaceSize;
use grana_editor::Message;
use grana_gpu::Gpu;
use grana_ui::Ui;
use winit::window::Window;

pub(super) struct Surface {
    pub(super) window: Arc<Window>,
    pub(super) ui: Ui<Message>,
    gpu: Gpu,
    size: SurfaceSize,
}

impl Surface {
    pub(super) fn new(window: Arc<Window>, gpu: Gpu, ui: Ui<Message>) -> Self {
        let size = SurfaceSize::new(gpu.size(), window.scale_factor() as f32)
            .expect("a window opens with a surface");
        let mut ui = ui;
        ui.resize(size.physical());
        Self {
            window,
            ui,
            gpu,
            size,
        }
    }

    pub(super) fn size(&self) -> SurfaceSize {
        self.size
    }

    pub(super) fn synchronize(&mut self) -> SurfaceSize {
        let inner = self.window.inner_size();
        if inner.width == 0 || inner.height == 0 {
            return self.size;
        }
        let size = SurfaceSize::new(
            [inner.width, inner.height],
            self.window.scale_factor() as f32,
        )
        .expect("a window holds a surface");
        if size == self.size {
            return self.size;
        }
        self.gpu.resize(size.physical());
        self.ui.resize(size.physical());
        self.size = size;
        size
    }

    pub(super) fn device(&self) -> &wgpu::Device {
        &self.gpu.device
    }

    pub(super) fn queue(&self) -> &wgpu::Queue {
        &self.gpu.queue
    }

    pub(super) fn lost(&self) -> Option<String> {
        self.gpu.lost()
    }

    pub(super) fn acquire(&mut self) -> Option<wgpu::SurfaceTexture> {
        self.gpu.acquire()
    }

    pub(super) fn present(&self, frame: wgpu::SurfaceTexture) {
        self.gpu.queue.present(frame);
    }
}
