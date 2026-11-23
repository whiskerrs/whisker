use std::os::fd::{FromRawFd, OwnedFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ash::vk;
use whisker_surface::{DmabufFrame, DmabufPlane, DmabufSink, FrameInfo};

use crate::{Gpu, Renderer, log};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const VK_FORMAT: vk::Format = vk::Format::R8G8B8A8_SRGB;
// DRM_FORMAT_ABGR8888 stores R, G, B, A bytes in memory order, like `FORMAT`.
const FOURCC: u32 = u32::from_le_bytes(*b"AB24");
const POOL_SIZE: usize = 3;

/// Presents through the Host's dmabuf sink, exporting GPU images where the driver and Host allow it
/// and copying pixels back otherwise.
pub(crate) struct Frames {
    sink: DmabufSink,
    buffers: Buffers,
}

enum Buffers {
    None,
    Exported(Vec<Exported>),
    Readback {
        texture: wgpu::Texture,
        buffer: wgpu::Buffer,
        stride: u32,
    },
}

struct Exported {
    texture: wgpu::Texture,
    fd: OwnedFd,
    modifier: u64,
    planes: Vec<(u32, u32)>,
    busy: Arc<AtomicBool>,
}

impl Frames {
    pub(crate) fn new(sink: DmabufSink) -> Self {
        Self {
            sink,
            buffers: Buffers::None,
        }
    }

    pub(crate) fn format(&self) -> wgpu::TextureFormat {
        FORMAT
    }

    pub(crate) fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) {
        self.buffers = match export_pool(gpu, &self.sink, width, height) {
            Ok(pool) => Buffers::Exported(pool),
            Err(reason) => {
                log(&format!(
                    "whisker-wgpu: copying frames back to the CPU: {reason}"
                ));
                readback(gpu, width, height)
            }
        };
    }

    pub(crate) fn draw(&mut self, gpu: &Gpu, renderer: &mut impl Renderer, frame: FrameInfo) {
        match &self.buffers {
            Buffers::None => {}
            Buffers::Exported(pool) => {
                // GTK still shows every buffer, so this frame has nowhere to go.
                let Some(buffer) = pool
                    .iter()
                    .find(|buffer| !buffer.busy.load(Ordering::Acquire))
                else {
                    return;
                };
                let view = buffer.texture.create_view(&Default::default());
                renderer.render(gpu, &view, frame);
                // The Host reads the image without a fence, so it must be finished first.
                if gpu
                    .device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .is_err()
                {
                    return;
                }
                if self.sink.present(buffer.frame()) {
                    return;
                }
                buffer.busy.store(false, Ordering::Release);
                log("whisker-wgpu: the Host rejected a dmabuf; copying frames back to the CPU");
                let size = buffer.texture.size();
                self.buffers = readback(gpu, size.width, size.height);
            }
            Buffers::Readback {
                texture,
                buffer,
                stride,
            } => {
                let view = texture.create_view(&Default::default());
                renderer.render(gpu, &view, frame);
                let size = texture.size();
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                encoder.copy_texture_to_buffer(
                    texture.as_image_copy(),
                    wgpu::TexelCopyBufferInfo {
                        buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(*stride),
                            rows_per_image: None,
                        },
                    },
                    size,
                );
                gpu.queue.submit([encoder.finish()]);
                let slice = buffer.slice(..);
                slice.map_async(wgpu::MapMode::Read, |_| {});
                if gpu
                    .device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .is_err()
                {
                    return;
                }
                if let Ok(pixels) = slice.get_mapped_range() {
                    self.sink
                        .present_pixels(size.width, size.height, *stride, &pixels);
                }
                buffer.unmap();
            }
        }
    }
}

impl Exported {
    /// Marks the buffer busy until the Host releases the returned frame.
    fn frame(&self) -> DmabufFrame {
        self.busy.store(true, Ordering::Release);
        let busy = self.busy.clone();
        let size = self.texture.size();
        DmabufFrame {
            width: size.width,
            height: size.height,
            fourcc: FOURCC,
            modifier: self.modifier,
            planes: self
                .planes
                .iter()
                .filter_map(|&(offset, stride)| {
                    Some(DmabufPlane {
                        fd: self.fd.try_clone().ok()?,
                        offset,
                        stride,
                    })
                })
                .collect(),
            release: Some(Box::new(move || busy.store(false, Ordering::Release))),
        }
    }
}

fn readback(gpu: &Gpu, width: u32, height: u32) -> Buffers {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("whisker-wgpu readback"),
        size: extent(width, height),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let stride = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("whisker-wgpu readback"),
        size: u64::from(stride) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    Buffers::Readback {
        texture,
        buffer,
        stride,
    }
}

fn extent(width: u32, height: u32) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    }
}

fn export_pool(
    gpu: &Gpu,
    sink: &DmabufSink,
    width: u32,
    height: u32,
) -> Result<Vec<Exported>, String> {
    let accepted = sink.modifiers(FOURCC);
    if accepted.is_empty() {
        return Err("the Host imports no AB24 dmabufs".into());
    }
    let mut pool = Vec::with_capacity(POOL_SIZE);
    for _ in 0..POOL_SIZE {
        let (raw, fd, modifier, planes) = {
            // SAFETY: the guard is dropped before the device is used through wgpu again.
            let hal = unsafe { gpu.device.as_hal::<wgpu::hal::api::Vulkan>() }
                .ok_or("the adapter is not Vulkan")?;
            let image = Image::new(&hal, &accepted, width, height)?;
            image.into_texture(&hal)
        };
        // SAFETY: `raw` was created from this device with `FORMAT`, the size above, and
        // `COLOR_TARGET` usage, matching this descriptor.
        let texture = unsafe {
            gpu.device
                .create_texture_from_hal::<wgpu::hal::api::Vulkan>(
                    raw,
                    &wgpu::TextureDescriptor {
                        label: Some("whisker-wgpu dmabuf"),
                        size: extent(width, height),
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: FORMAT,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    },
                    wgpu::TextureUses::UNINITIALIZED,
                )
        };
        pool.push(Exported {
            texture,
            fd,
            modifier,
            planes,
            busy: Arc::new(AtomicBool::new(false)),
        });
    }
    Ok(pool)
}

/// An exportable image and its memory, destroyed on drop unless handed to wgpu.
struct Image {
    device: ash::Device,
    image: vk::Image,
    memory: vk::DeviceMemory,
    fd: Option<OwnedFd>,
    modifier: u64,
    planes: Vec<(u32, u32)>,
    size: (u32, u32),
}

impl Image {
    fn new(
        hal: &wgpu::hal::vulkan::Device,
        accepted: &[u64],
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let extensions = hal.enabled_device_extensions();
        for extension in [
            ash::khr::external_memory_fd::NAME,
            ash::ext::external_memory_dma_buf::NAME,
            ash::ext::image_drm_format_modifier::NAME,
        ] {
            if !extensions.contains(&extension) {
                return Err(format!("the device lacks {extension:?}"));
            }
        }
        let instance = hal.shared_instance().raw_instance();
        let physical = hal.raw_physical_device();
        let device = hal.raw_device().clone();
        let supported = renderable_modifiers(instance, physical);
        let modifiers: Vec<u64> = supported
            .iter()
            .map(|&(modifier, _)| modifier)
            .filter(|modifier| accepted.contains(modifier))
            .collect();
        if modifiers.is_empty() {
            return Err("the GPU renders to no modifier the Host imports".into());
        }

        let mut modifier_list =
            vk::ImageDrmFormatModifierListCreateInfoEXT::default().drm_format_modifiers(&modifiers);
        let mut external = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(VK_FORMAT)
            .extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
            .usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .push_next(&mut external)
            .push_next(&mut modifier_list);
        // SAFETY: `info` is a valid 2D image description whose chained structs outlive the call.
        let image = unsafe { device.create_image(&info, None) }
            .map_err(|error| format!("vkCreateImage: {error}"))?;
        let mut this = Self {
            device,
            image,
            memory: vk::DeviceMemory::null(),
            fd: None,
            modifier: 0,
            planes: Vec::new(),
            size: (width, height),
        };
        this.fd = Some(this.allocate(instance, physical, &supported)?);
        Ok(this)
    }

    /// Binds dedicated exportable memory and returns its dmabuf fd.
    fn allocate(
        &mut self,
        instance: &ash::Instance,
        physical: vk::PhysicalDevice,
        supported: &[(u64, u32)],
    ) -> Result<OwnedFd, String> {
        let device = &self.device;
        let modifier_fn = ash::ext::image_drm_format_modifier::Device::new(instance, device);
        let mut properties = vk::ImageDrmFormatModifierPropertiesEXT::default();
        // SAFETY: the image was created with DRM format modifier tiling.
        unsafe {
            modifier_fn.get_image_drm_format_modifier_properties(self.image, &mut properties)
        }
        .map_err(|error| format!("vkGetImageDrmFormatModifierPropertiesEXT: {error}"))?;
        self.modifier = properties.drm_format_modifier;
        let plane_count = supported
            .iter()
            .find(|&&(modifier, _)| modifier == self.modifier)
            .map_or(1, |&(_, planes)| planes);

        // SAFETY: the image is valid and not yet bound.
        let requirements = unsafe { device.get_image_memory_requirements(self.image) };
        // SAFETY: `physical` belongs to `instance`.
        let memory_properties = unsafe { instance.get_physical_device_memory_properties(physical) };
        let memory_type = (0..memory_properties.memory_type_count)
            .find(|&index| {
                requirements.memory_type_bits & (1 << index) != 0
                    && memory_properties.memory_types[index as usize]
                        .property_flags
                        .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
            })
            .ok_or("no device-local memory type fits the image")?;
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(self.image);
        let mut export = vk::ExportMemoryAllocateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let allocation = vk::MemoryAllocateInfo::default()
            .allocation_size(requirements.size)
            .memory_type_index(memory_type)
            .push_next(&mut export)
            .push_next(&mut dedicated);
        // SAFETY: `allocation` and its chained structs are valid and outlive the call.
        self.memory = unsafe { device.allocate_memory(&allocation, None) }
            .map_err(|error| format!("vkAllocateMemory: {error}"))?;
        // SAFETY: the memory was allocated for this image with its requirements.
        unsafe { device.bind_image_memory(self.image, self.memory, 0) }
            .map_err(|error| format!("vkBindImageMemory: {error}"))?;

        const PLANES: [vk::ImageAspectFlags; 4] = [
            vk::ImageAspectFlags::MEMORY_PLANE_0_EXT,
            vk::ImageAspectFlags::MEMORY_PLANE_1_EXT,
            vk::ImageAspectFlags::MEMORY_PLANE_2_EXT,
            vk::ImageAspectFlags::MEMORY_PLANE_3_EXT,
        ];
        self.planes = PLANES
            .iter()
            .take(plane_count as usize)
            .map(|&aspect| {
                // SAFETY: the plane index is below the modifier's plane count.
                let layout = unsafe {
                    device.get_image_subresource_layout(
                        self.image,
                        vk::ImageSubresource {
                            aspect_mask: aspect,
                            mip_level: 0,
                            array_layer: 0,
                        },
                    )
                };
                (layout.offset as u32, layout.row_pitch as u32)
            })
            .collect();

        let fd_fn = ash::khr::external_memory_fd::Device::new(instance, device);
        let info = vk::MemoryGetFdInfoKHR::default()
            .memory(self.memory)
            .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        // SAFETY: the memory was allocated exportable as a dmabuf.
        let fd = unsafe { fd_fn.get_memory_fd(&info) }
            .map_err(|error| format!("vkGetMemoryFdKHR: {error}"))?;
        // SAFETY: vkGetMemoryFdKHR transfers ownership of a new fd to the caller.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }

    /// Hands the image and memory to wgpu, which frees them when its texture is dropped.
    fn into_texture(
        mut self,
        hal: &wgpu::hal::vulkan::Device,
    ) -> (wgpu::hal::vulkan::Texture, OwnedFd, u64, Vec<(u32, u32)>) {
        let device = self.device.clone();
        let fd = self.fd.take().expect("allocated in Image::new");
        let planes = std::mem::take(&mut self.planes);
        let image = std::mem::replace(&mut self.image, vk::Image::null());
        let memory = std::mem::replace(&mut self.memory, vk::DeviceMemory::null());
        let (width, height) = self.size;
        let descriptor = wgpu::hal::TextureDescriptor {
            label: Some("whisker-wgpu dmabuf"),
            size: extent(width, height),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUses::COLOR_TARGET,
            memory_flags: wgpu::hal::MemoryFlags::empty(),
            view_formats: Vec::new(),
        };
        // SAFETY: the image matches `descriptor`, and the callback frees the image and memory only
        // after wgpu has dropped the texture. Exported dmabufs keep their own reference.
        let texture = unsafe {
            hal.texture_from_raw(
                image,
                &descriptor,
                Some(Box::new(move || {
                    device.destroy_image(image, None);
                    device.free_memory(memory, None);
                })),
                wgpu::hal::vulkan::TextureMemory::External,
            )
        };
        (texture, fd, self.modifier, planes)
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        // SAFETY: a non-null image or memory never reached wgpu, so nothing else uses it.
        unsafe {
            if self.image != vk::Image::null() {
                self.device.destroy_image(self.image, None);
            }
            if self.memory != vk::DeviceMemory::null() {
                self.device.free_memory(self.memory, None);
            }
        }
    }
}

/// DRM modifiers the GPU can render `VK_FORMAT` to, with each one's plane count.
fn renderable_modifiers(instance: &ash::Instance, physical: vk::PhysicalDevice) -> Vec<(u64, u32)> {
    let mut list = vk::DrmFormatModifierPropertiesListEXT::default();
    let mut properties = vk::FormatProperties2::default().push_next(&mut list);
    // SAFETY: `physical` belongs to `instance`; the first call only counts the modifiers.
    unsafe {
        instance.get_physical_device_format_properties2(physical, VK_FORMAT, &mut properties)
    };
    let mut modifiers = vec![
        vk::DrmFormatModifierPropertiesEXT::default();
        list.drm_format_modifier_count as usize
    ];
    let mut list = vk::DrmFormatModifierPropertiesListEXT::default()
        .drm_format_modifier_properties(&mut modifiers);
    let mut properties = vk::FormatProperties2::default().push_next(&mut list);
    // SAFETY: the list has room for every modifier the first call counted.
    unsafe {
        instance.get_physical_device_format_properties2(physical, VK_FORMAT, &mut properties)
    };
    modifiers
        .iter()
        .filter(|properties| {
            properties
                .drm_format_modifier_tiling_features
                .contains(vk::FormatFeatureFlags::COLOR_ATTACHMENT)
        })
        .map(|properties| {
            (
                properties.drm_format_modifier,
                properties.drm_format_modifier_plane_count,
            )
        })
        .collect()
}
