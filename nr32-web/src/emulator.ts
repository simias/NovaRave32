import init, { NoRa32 } from './wasm/novarave32.js';
import { GlContext } from './gl.js';

import noRaVertexShader from './shaders/nora.vert.glsl?raw';
import noRaFragmentShader from './shaders/nora.frag.glsl?raw';
import screenVertexShader from './shaders/screen.vert.glsl?raw';
import screenFragmentShader from './shaders/screen.frag.glsl?raw';

export class Emulator {
  canvas: HTMLCanvasElement;
  m: NoRa32;
  // Context for rendering NoRa draw commands to an off-screen buffer
  noRaContext: GlContext;
  // Context for displaying the off-screen buffer to the canvas
  screenContext: GlContext;
  wasm: Awaited<ReturnType<typeof init>>;

  private constructor(canvas: HTMLCanvasElement, wasm: Awaited<ReturnType<typeof init>>) {
    this.wasm = wasm;

    this.canvas = canvas;
    this.m = new NoRa32();

    this.canvas.style.imageRendering = 'pixelated';

    const gl = this.canvas.getContext('webgl2', { antialias: false });
    if (!gl) {
      throw new Error('WebGL2 not supported in this environment.');
    }

    this.noRaContext = new GlContext(gl);
    this.noRaContext.compileShaders(noRaVertexShader, noRaFragmentShader);

    const u8Buffer = this.noRaContext.mapBuffer(
      { location: 'a_color', type: gl.UNSIGNED_BYTE, size: 4 },
      { location: 'a_projection_index', type: gl.UNSIGNED_BYTE, size: 1 },
    );

    const i16Buffer = this.noRaContext.mapBuffer({
      location: 'a_position',
      type: gl.SHORT,
      size: 3,
    });

    const projectionsLoc = this.noRaContext.getUniformLocation('u_projections');

    // Framebuffer used for off-screen rendering
    const noRaFbo = gl.createFramebuffer();

    const noRaFbTex = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, noRaFbTex);
    gl.texImage2D(
      gl.TEXTURE_2D,
      0,
      gl.RGBA,
      canvas.width,
      canvas.height,
      0,
      gl.RGBA,
      gl.UNSIGNED_BYTE,
      null,
    );
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);

    // Depth buffer
    const noRaFbDepth = gl.createRenderbuffer();
    gl.bindRenderbuffer(gl.RENDERBUFFER, noRaFbDepth);
    gl.renderbufferStorage(gl.RENDERBUFFER, gl.DEPTH_COMPONENT16, canvas.width, canvas.height);

    const noRaBind = () => {
      this.noRaContext.bind();

      gl.bindFramebuffer(gl.FRAMEBUFFER, noRaFbo);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, noRaFbTex, 0);
      gl.framebufferRenderbuffer(gl.FRAMEBUFFER, gl.DEPTH_ATTACHMENT, gl.RENDERBUFFER, noRaFbDepth);

      gl.enable(gl.BLEND);
      gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
      gl.enable(gl.DEPTH_TEST);

      gl.clearColor(0.0, 0.0, 0.0, 1.0);
      gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    };

    this.screenContext = new GlContext(gl);
    this.screenContext.compileShaders(screenVertexShader, screenFragmentShader);

    const screenTextureLoc = this.screenContext.getUniformLocation('u_screen_texture');

    // Prepare the hardcoded data for the screen shader
    {
      const quadVert = new Float32Array([
        // Bottom-left (x, y, u, v)
        -1, -1, 0, 0,
        // Bottom-right
        1, -1, 1, 0,
        // Top-left
        -1, 1, 0, 1,
        // Top-right
        1, 1, 1, 1,
      ]);

      const quadBuf = this.screenContext.mapBuffer(
        { location: 'a_position', type: gl.FLOAT, size: 2 },
        { location: 'a_tex_coord', type: gl.FLOAT, size: 2 },
      );
      gl.bindBuffer(gl.ARRAY_BUFFER, quadBuf);
      gl.bufferData(gl.ARRAY_BUFFER, quadVert, gl.STATIC_DRAW);
    }

    noRaBind();

    this.m.on_draw_triangles(
      (mat_f32_ptr: number, mat_count: number, i16_ptr: number, u8_ptr: number, count: number) => {
        const i16Data = new Int16Array(wasm.memory.buffer, i16_ptr, count * 3);
        const u8Data = new Uint8Array(wasm.memory.buffer, u8_ptr, count * 5);
        const matdata = new Float32Array(wasm.memory.buffer, mat_f32_ptr, mat_count * 16);

        gl.bindBuffer(gl.ARRAY_BUFFER, i16Buffer);
        gl.bufferData(gl.ARRAY_BUFFER, i16Data, gl.STREAM_DRAW);

        gl.bindBuffer(gl.ARRAY_BUFFER, u8Buffer);
        gl.bufferData(gl.ARRAY_BUFFER, u8Data, gl.STREAM_DRAW);

        gl.uniformMatrix4fv(projectionsLoc, false, matdata);

        gl.drawArrays(gl.TRIANGLES, 0, count);
      },
    );

    this.m.on_display_framebuffer(() => {
      this.screenContext.bind();

      // We draw to the canvas
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);

      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, noRaFbTex);
      gl.uniform1i(screenTextureLoc, 0);

      gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);

      // Rebind the normal context for the next frame
      noRaBind();
    });
  }

  static async build(canvas: HTMLCanvasElement): Promise<Emulator> {
    const wasm = await init();

    return new Emulator(canvas, wasm);
  }

  loadRom(rom: Uint8Array) {
    this.m.load_rom(rom);
  }

  runFrame() {
    this.m.run_frame();
  }
}
