export class GlContext {
  gl: WebGL2RenderingContext;
  vao: WebGLVertexArrayObject;
  program: WebGLProgram;
  attributes: { [id: string]: WebGLActiveInfo };

  constructor(gl: WebGL2RenderingContext) {
    this.gl = gl;

    this.vao = gl.createVertexArray();
    this.program = gl.createProgram();
    this.attributes = {};
  }

  compileShaders(vertexShader: string, fragmentShader: string) {
    const gl = this.gl;

    const compileShader = (type: number, source: string) => {
      const shader = gl.createShader(type);
      if (!shader) {
        throw new Error("Can't create shader");
      }

      gl.shaderSource(shader, source);
      gl.compileShader(shader);

      if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
        const err = `Shader compilation failed: ${gl.getShaderInfoLog(shader)}`;
        gl.deleteShader(shader);
        throw new Error(err);
      }

      return shader;
    };

    const vs = compileShader(gl.VERTEX_SHADER, vertexShader);
    const fs = compileShader(gl.FRAGMENT_SHADER, fragmentShader);

    gl.attachShader(this.program, vs);
    gl.attachShader(this.program, fs);
    gl.linkProgram(this.program);

    // Cache attribute info
    const active_attrs = gl.getProgramParameter(this.program, gl.ACTIVE_ATTRIBUTES);

    for (let attr = 0; attr < active_attrs; attr++) {
      const a = gl.getActiveAttrib(this.program, attr);

      if (a) {
        this.attributes[a.name] = a;
      }
    }
  }

  bind() {
    this.gl.bindVertexArray(this.vao);
    this.gl.useProgram(this.program);
  }

  getUniformLocation(name: string): WebGLUniformLocation {
    const uniform = this.gl.getUniformLocation(this.program, name);

    if (uniform) {
      return uniform;
    } else {
      throw new Error(`Unknown uniform ${name}`);
    }
  }

  // Create and maps an OpenGL buffer to this.vao.
  mapBuffer(...defs: BufferDefinition[]): WebGLBuffer {
    const gl = this.gl;
    const GL_TYPE_BYTE_SIZES: Record<number, number> = {
      [gl.BYTE]: 1,
      [gl.UNSIGNED_BYTE]: 1,
      [gl.SHORT]: 2,
      [gl.UNSIGNED_SHORT]: 2,
      [gl.INT]: 4,
      [gl.UNSIGNED_INT]: 4,
      [gl.FLOAT]: 4,
    };

    const GL_TYPE_IS_FLOAT: Record<number, boolean> = {
      [gl.UNSIGNED_INT]: false,
      [gl.UNSIGNED_INT_VEC4]: false,
      [gl.INT_VEC3]: false,
      [gl.FLOAT_VEC2]: true,
    };

    let stride = 0;

    for (const d of defs) {
      const size = GL_TYPE_BYTE_SIZES[d.type];
      if (!size) {
        throw new Error(`Don't know the size of ${d.type.toString(16)}`);
      }

      stride += size * d.size;
    }

    if (stride == 0) {
      throw new Error('Attempted to create a GL Buffer with 0-sized attributes');
    }

    this.bind();

    const buf = gl.createBuffer();
    this.gl.bindBuffer(gl.ARRAY_BUFFER, buf);

    let offset = 0;

    for (const d of defs) {
      const loc = gl.getAttribLocation(this.program, d.location);

      gl.enableVertexAttribArray(loc);

      const attr = this.attributes[d.location];

      if (!attr) {
        gl.deleteBuffer(buf);
        throw new Error(`Unknown location ${d.location}`);
      }

      const isFloat = GL_TYPE_IS_FLOAT[attr.type];

      if (isFloat === undefined) {
        gl.deleteBuffer(buf);
        throw new Error(`Unknown location type ${attr.type.toString(16)}`);
      }

      if (isFloat) {
        gl.vertexAttribPointer(loc, d.size, d.type, !d.no_normalize, stride, offset);
      } else {
        gl.vertexAttribIPointer(loc, d.size, d.type, stride, offset);
      }

      offset += (GL_TYPE_BYTE_SIZES[d.type] ?? 0) * d.size;
    }

    return buf;
  }
}

type BufferDefinition = {
  // Location in the program
  location: string;
  // gl.SHORT, gl.FLOAT, gl.UNSIGNED_BYTE ...
  type: number;
  // Number of components
  size: number;
  // For float shader attributes, set to true if the value shouldn't be
  // normalized. Does nothing for integer attributes.
  no_normalize?: boolean;
};
