use anyhow::Result;
use byteorder::{LittleEndian, WriteBytesExt};
use std::io::Write;
use std::path::Path;

pub struct Model {
    /// Scaling factor applied to the vertex coordinates
    ///
    /// Technically we could scale differently on all 3 axes (may be beneficial for very narrow
    /// models in order not to lose resolution on the narrow axes) but for now I prefer to keep it
    /// simple
    scale: f32,

    /// Origin used for output coordinates. Used to adjust the coordinates of the source model so
    /// that coordinate `c` in the source model becomes `c - origin` in the output file.
    ///
    /// This is so that we don't lose too much precision for off-center models that wouldn't use a
    /// large chunk of the integer range for the output
    origin: [f32; 3],

    /// Vertices (as loaded from the source file, any adjustment/scaling/... is done when dumping
    vertices: Vec<Vertex>,

    /// Triangle indices. Always treated as a triangle strip, `None` used for restarts
    indices: Vec<Option<u32>>,
}

impl Model {
    /// Loads a gltf model file with the provided options
    pub fn load_with_options<P: AsRef<Path>>(gltf_path: P, opts: ModelOptions) -> Result<Model> {
        let path = gltf_path.as_ref();

        info!("Loading model from `{}`", path.display());

        let (gltf, buffers, _images) = gltf::import(path)?;

        debug!("Found {} mesh(es):", gltf.meshes().len());

        // Display all meshes
        for mesh in gltf.meshes() {
            debug!(
                "Mesh #{} `{}` with {} primitives",
                mesh.index(),
                mesh.name().unwrap_or("<NONAME>"),
                mesh.primitives().len()
            );
        }

        for mesh in gltf.meshes() {
            if mesh.index() != opts.mesh {
                continue;
            }

            return Model::from_gltf_mesh(opts, &gltf, mesh, &buffers);
        }

        // Ok(Model { opts })
        bail!("Mesh {} not found!", opts.mesh);
    }

    fn from_gltf_mesh<'a>(
        opts: ModelOptions,
        _gltf: &'a gltf::Document,
        mesh: gltf::Mesh<'a>,
        buffers: &[gltf::buffer::Data],
    ) -> Result<Model> {
        info!(
            "Loading mesh #{} `{}` with {} primitives",
            mesh.index(),
            mesh.name().unwrap_or("<NONAME>"),
            mesh.primitives().len()
        );

        for prim in mesh.primitives() {
            debug!("Primitive #{} {:?}", prim.index(), prim.mode());

            debug!("- Bounding box: {:?}", prim.bounding_box());
        }

        let (bbmin, bbmax) = {
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];

            for prim in mesh.primitives() {
                let bbox = prim.bounding_box();
                for i in 0..3 {
                    min[i] = min[i].min(bbox.min[i]);
                    max[i] = max[i].max(bbox.max[i]);
                }
            }

            let w = max[0] - min[0];
            let h = max[1] - min[1];
            let d = max[2] - min[2];

            debug!("Mesh dimensions: {:.3e} x {:.3e} x {:.3e}", w, h, d);

            (min, max)
        };

        if !bbmin.iter().all(|c| c.is_finite()) || !bbmax.iter().all(|c| c.is_finite()) {
            bail!("Got non-finite dimensions while calculating the mesh bounding box");
        }

        let origin = if opts.recenter {
            let w = bbmax[0] - bbmin[0];
            let h = bbmax[1] - bbmin[1];
            let d = bbmax[2] - bbmin[2];

            let c = [bbmin[0] + w / 2., bbmin[1] + h / 2., bbmin[2] + d / 2.];

            [
                to_fp32_compatible(c[0]),
                to_fp32_compatible(c[1]),
                to_fp32_compatible(c[2]),
            ]
        } else {
            // Keep origin where it is
            [0.; 3]
        };

        debug!("- Origin: {:?}", origin);

        // Returns the maximum absolute value any coordinate of a point inside the mesh's bounding
        // box can take
        let coords_max = {
            let max = [
                bbmax[0] - origin[0],
                bbmax[1] - origin[1],
                bbmax[2] - origin[2],
            ];

            let min = [
                bbmin[0] - origin[0],
                bbmin[1] - origin[1],
                bbmin[2] - origin[2],
            ];

            let coords = [
                max[0].abs(),
                max[1].abs(),
                max[2].abs(),
                min[0].abs(),
                min[1].abs(),
                min[2].abs(),
            ];

            coords
                .iter()
                .max_by(|&a, &b| a.total_cmp(b))
                .cloned()
                .unwrap()
        };

        // How much we can scale without overflowing based on the bounding box
        let scale_max = f32::from(INT_COORDS_MAX) / coords_max;

        debug!("Scaling max before clipping: {}", scale_max);

        let scale = match opts.scale {
            Some(s) => {
                if s > scale_max {
                    warn!("The scaling factor is too large and will probably cause clipping (safe max: {})",
                        scale_max);
                }

                s
            }
            None => {
                // Make sure the scale can be accurately represented in the scaling matrix (where
                // we'll store 1/scale)
                //
                // The small offset is to make sure that to_fp32_compatible will not round iscale
                // down, which would result in vertex clipping
                let iscale = to_fp32_compatible(1. / scale_max + (0.5 / 65536.));
                1. / iscale
            }
        };

        debug!("Scale factor: {:.3}", scale);

        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        // Now we can load the primitives
        for prim in mesh.primitives() {
            debug!("Loading vertex data for primitive #{}", prim.index());
            if prim.mode() != gltf::mesh::Mode::Triangles {
                warn!(
                    "Primitive mode `{:?}` is not supported, primitive #{} ignored",
                    prim.mode(),
                    prim.index()
                );

                continue;
            }

            let mat = prim.material();

            debug!(
                "Material: #{} `{:?}`",
                mat.index().map(|u| u as isize).unwrap_or(-1),
                mat.name().unwrap_or("<NONAME>")
            );

            let default_color = {
                let col = mat.pbr_metallic_roughness().base_color_factor();

                [
                    (col[0] * 255.).clamp(0., 255.) as u8,
                    (col[1] * 255.).clamp(0., 255.) as u8,
                    (col[2] * 255.).clamp(0., 255.) as u8,
                ]
            };

            debug!("Material color: {:?}", default_color);

            // XXX We could use "emissive_strength" instead but Blender 4.3.2 doesn't seem to
            // export it. As a workaround I just see if a non-black emissive factor is used.
            let is_emissive = mat.emissive_factor().iter().sum::<f32>() > 0.01;

            if is_emissive {
                debug!("Material is emissive");
            }

            let is_strip = prim.mode() == gltf::mesh::Mode::TriangleStrip;

            // let reader = prim.reader(|b| Some(&buffers[b.index()]));
            let reader = prim.reader(|b| buffers.get(b.index()).map(|d| d as &[u8]));

            // Since we store all mesh primitives in one array, we have to offset the indices of
            // subsequent primitives
            let index_offset = vertices.len();

            match reader.read_positions() {
                None => {
                    warn!("Primitive #{} has no position data", prim.index());
                    continue;
                }
                Some(pp) => {
                    for pos in pp {
                        let mut v = Vertex::from_position_color(pos, default_color);

                        v.set_emissive(is_emissive);

                        vertices.push(v);
                    }
                }
            }

            match reader.read_indices() {
                None => {
                    warn!("Primitive #{} has no index data", prim.index());
                    continue;
                }
                Some(ii) => {
                    for (i, index) in ii.into_u32().enumerate() {
                        indices.push(Some((index_offset as u32) + index));
                        if !is_strip && i % 3 == 2 {
                            // We treat everything as a strip so we insert a restart when using
                            // normal triangles
                            indices.push(None);
                        }
                    }
                }
            }

            // What's the meaning of the "set"?
            if let Some(cc) = reader.read_colors(0) {
                debug!("Primitive #{} has colors", prim.index());

                for (i, c) in cc.into_rgb_u8().enumerate() {
                    if let Some(v) = vertices.get_mut(index_offset + i) {
                        v.set_color(c);
                    }
                }
            }
        }

        Ok(Model {
            scale,
            origin,
            vertices,
            indices,
        })
    }

    pub fn options() -> ModelOptions {
        ModelOptions::new()
    }

    pub fn triangle_count(&self) -> usize {
        let mut count = 0;

        let mut series: usize = 0;

        // Iterate through the strip and count the triangles
        for i in &self.indices {
            match i {
                // Restart
                None => series = 0,
                Some(_) => {
                    series += 1;

                    if series >= 3 {
                        count += 1;
                    }
                }
            }
        }

        count
    }

    pub fn dump_nr3d<W: Write>(&self, w: &mut W) -> Result<()> {
        let wu32 = |w: &mut W, v| w.write_u32::<LittleEndian>(v);

        let scale_f32 = |v: f32| -> i32 {
            let min = i32::MIN as f32;
            let max = i32::MAX as f32;

            (v * self.scale).round().clamp(min, max) as i32
        };

        let scale_coords = |c: [f32; 3]| -> [i32; 3] {
            [
                scale_f32(c[0] - self.origin[0]),
                scale_f32(c[1] - self.origin[1]),
                scale_f32(c[2] - self.origin[2]),
            ]
        };

        // File format identifier (NOP GPU command)
        //
        // bits [16:24] could be used for flags later
        wu32(w, 0x0000524e)?;

        // Matrix header
        {
            // Put model matrix in M3
            let m = 3;

            // Matrix identity
            wu32(w, (0x10 << 24) | (m << 12))?;

            // Translation factor
            for (row, &t) in self.origin.iter().enumerate() {
                let fpt = (t * 65536.).round().clamp(i32::MIN as f32, i32::MAX as f32) as i32;

                if fpt != 0 {
                    wu32(
                        w,
                        (0x10 << 24) | (1 << 16) | (m << 12) | (3 << 4) | (row as u32),
                    )?;

                    wu32(w, fpt as u32)?;
                }
            }

            // Scaling factor
            let iscale = (65536. / self.scale)
                .abs()
                .round()
                .clamp(1., i32::MAX as f32) as u32;
            if iscale != 1 {
                for p in 0..3 {
                    wu32(w, (0x10 << 24) | (1 << 16) | (m << 12) | (p << 4) | p)?;
                    wu32(w, iscale)?;
                }
            }

            // M0 = M0 * M3
            let mo = 0;
            let ma = 0;
            let mb = 3;

            wu32(w, (0x10 << 24) | (0x02 << 16) | (mo << 12) | (ma << 4) | mb)?;
        }

        // Add an empty word (NOP for the GPU) to delineate the start of the vertex data. This way
        // we can easily skip the matrix setup if we don't need it later
        wu32(w, 0x0000_0042)?;

        let mut clip_count = 0;

        let mut series: usize = 0;
        for (i, &index) in self.indices.iter().enumerate() {
            match index {
                None => series = 0,
                Some(index) => {
                    series += 1;

                    if series < 3 {
                        // No full triangle yet
                        continue;
                    }

                    if series > 3 {
                        warn!("Strip dumping not implemented!");
                    }

                    let i0 = self.indices[i - 2].unwrap();
                    let i1 = self.indices[i - 1].unwrap();
                    let i2 = index;

                    let v0 = self
                        .vertices
                        .get(i0 as usize)
                        .ok_or_else(|| anyhow!("got invalid vertex index"))?;
                    let v1 = self
                        .vertices
                        .get(i1 as usize)
                        .ok_or_else(|| anyhow!("got invalid vertex index"))?;
                    let v2 = self
                        .vertices
                        .get(i2 as usize)
                        .ok_or_else(|| anyhow!("got invalid vertex index"))?;

                    let bgr888 = |c: [u8; 3]| -> u32 {
                        (c[0] as u32) | ((c[1] as u32) << 8) | ((c[2] as u32) << 16)
                    };

                    let c0 = bgr888(v0.col);
                    let c1 = bgr888(v1.col);
                    let c2 = bgr888(v2.col);

                    let needs_gouraud = c0 != c1 || c0 != c2;

                    let blend_mode = if needs_gouraud { 2 } else { 0 };

                    let p0 = scale_coords(v0.pos);
                    let p1 = scale_coords(v1.pos);
                    let p2 = scale_coords(v2.pos);

                    let is_clipped = |&coord: &i32| -> bool {
                        if coord < i32::from(INT_COORDS_MIN) || coord > i32::from(INT_COORDS_MAX) {
                            error!("clip {}", coord);
                            true
                        } else {
                            false
                        }
                    };

                    if p0.iter().any(is_clipped)
                        || p1.iter().any(is_clipped)
                        || p2.iter().any(is_clipped)
                    {
                        clip_count += 1;
                        continue;
                    }

                    let cmd = (0x40 << 24) | (blend_mode << 25) | c0;
                    wu32(w, cmd)?;

                    let xyz = |w: &mut W, pos: [i32; 3]| {
                        w.write_i16::<LittleEndian>(pos[2] as i16)?;
                        w.write_i16::<LittleEndian>(0)?;
                        w.write_i16::<LittleEndian>(pos[0] as i16)?;
                        w.write_i16::<LittleEndian>(pos[1] as i16)
                    };

                    xyz(w, p0)?;
                    if needs_gouraud {
                        wu32(w, c1)?;
                    }
                    xyz(w, p1)?;
                    if needs_gouraud {
                        wu32(w, c2)?;
                    }
                    xyz(w, p2)?;
                }
            }
        }

        if clip_count > 0 {
            warn!(
                "{} triangles have been clipped (try reducing the scale factor)",
                clip_count
            );
        }

        Ok(())
    }
}

#[derive(Clone)]
pub struct ModelOptions {
    pub keep_normals: bool,
    /// Mesh index to dump
    pub mesh: usize,
    pub scale: Option<f32>,
    pub recenter: bool,
}

impl ModelOptions {
    pub fn new() -> ModelOptions {
        ModelOptions {
            keep_normals: false,
            mesh: 0,
            scale: None,
            recenter: true,
        }
    }

    pub fn keep_normals(&mut self, keep: bool) -> &mut Self {
        self.keep_normals = keep;

        self
    }

    pub fn mesh(&mut self, mesh: usize) -> &mut Self {
        self.mesh = mesh;

        self
    }

    pub fn scale(&mut self, scale: Option<f32>) -> &mut Self {
        self.scale = scale;

        self
    }

    pub fn recenter(&mut self, recenter: bool) -> &mut Self {
        self.recenter = recenter;

        self
    }

    pub fn load<P: AsRef<Path>>(&self, gltf_path: P) -> Result<Model> {
        Model::load_with_options(gltf_path, self.clone())
    }
}

#[derive(Copy, Clone)]
struct Vertex {
    pos: [f32; 3],
    col: [u8; 3],
    emissive: bool,
}

impl Vertex {
    fn from_position_color(pos: [f32; 3], col: [u8; 3]) -> Vertex {
        Vertex {
            pos,
            col,
            emissive: false,
        }
    }

    fn set_color(&mut self, col: [u8; 3]) {
        self.col = col;
    }

    fn set_emissive(&mut self, emissive: bool) {
        self.emissive = emissive;
    }
}

/// Takes an f32 and returns the closest value that can be accurately represented in signed 16.16
/// fixed point
fn to_fp32_compatible(v: f32) -> f32 {
    let fp = (v * 65536.).round();

    let fp = fp.clamp(i32::MIN as f32, i32::MAX as f32);

    fp / 65536.
}

/// The max value we can represent in an NR3D coordinate
const INT_COORDS_MAX: i16 = i16::MAX;

/// The min value we can represent in an NR3D coordinate
const INT_COORDS_MIN: i16 = i16::MIN;
