#!/usr/bin/env bash

set -e

HELP_STRING=$(cat <<- END
	usage: build_wasm.sh

	Build script for combining a Macroquad project with wasm-bindgen,
	allowing integration with the greater wasm-ecosystem.

	example: ./build_wasm.sh

	This'll go through the following steps:

	    1. Build as target 'wasm32-unknown-unknown'.
	    2. Create the directory 'dist' if it doesn't already exist.
	    3. Run wasm-bindgen with output into the 'dist' directory.
	    4. Apply patches to the output js file (detailed here: https://github.com/not-fl3/macroquad/issues/212#issuecomment-835276147).
        5. Generate coresponding 'index.html' file.

	Author: Tom Solberg <me@sbg.dev>
	Edit: Nik codes <nik.code.things@gmail.com>
	Edit: Nobbele <realnobbele@gmail.com>
	Version: 0.2
END
)


die () {
    echo >&2 "Error: $@"
    echo >&2
    echo >&2 "$HELP_STRING"
    exit 1
}

# Parse primary commands
while [[ $# -gt 0 ]]
do
    key="$1"
    case $key in
        --release)
            RELEASE=yes
            shift
            ;;

        -h|--help)
            echo "$HELP_STRING"
            exit 0
            ;;

        *)
            POSITIONAL+=("$1")
            shift
            ;;
    esac
done

# Restore positionals
set -- "${POSITIONAL[@]}"

PROJECT_NAME="prpr"

HTML=$(cat <<- END
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <title>${PROJECT_NAME}</title>
    <style>
        html,
        body,
        canvas,
        #container {
            margin: 0px;
            padding: 0px;
            width: 100%;
            height: 100%;
            overflow: hidden;
            position: absolute;
            z-index: 0;
        }

        #container {
            position: relative;
        }
    </style>
</head>
<body style="margin: 0; padding: 0; height: 100vh; width: 100vw;">
    <div id="container" hidden>
        <div style="position: absolute; display: flex; justify-content: center; align-items: center; width: 100vw; height: 100%; flex-direction: column; z-index: 1;">
            <h1 id="status" style="color: white;">Loading WASM</h1>
        </div>
        <canvas onclick="full()" id="glcanvas" tabindex="1" hidden></canvas>
    </div>
    <!--<script src="https://not-fl3.github.io/miniquad-samples/mq_js_bundle.js"></script>-->
    <script src="./mq_js_bundle.js"></script>
    <script type="module">
        import init, { set_wasm } from "./${PROJECT_NAME}.js";
        async function impl_run() {
            gl.clearColorStencil = gl.clearStencil;
            GL.renderbuffers = [];
            gl.glGenRenderbuffers = function (n, ids) {
                _glGenObject(n, ids, 'createRenderbuffer', GL.renderbuffers, 'glGenRenderbuffers');
            };
            gl.glBindRenderbuffer = function (target, renderbuffer) {
                GL.validateGLObjectID(GL.renderbuffers, renderbuffer, 'glBindRenderbuffer', 'renderbuffer');

                gl.bindRenderbuffer(target, GL.renderbuffers[renderbuffer]);
            };
            gl.glRenderbufferStorageMultisample = function (target, samples, internalFormat, width, height) {
                gl.renderbufferStorageMultisample(target, samples, internalFormat, width, height);
            };
            gl.glFramebufferRenderbuffer = function (target, attachment, renderbuffertarget, renderbuffer) {
                gl.framebufferRenderbuffer(target, attachment, renderbuffertarget, GL.renderbuffers[renderbuffer]);
            };
            gl.glDrawBuffers = function (length, buffers) {
                gl.drawBuffers(buffers? getArray(buffers, Int32Array, length): null);
            };
            gl.glBlitFramebuffer = function (srcX0, srcY0, srcX1, srcY1, dstX0, dstY0, dstX1, dstY1, mask, filter) {
                gl.blitFramebuffer(srcX0, srcY0, srcX1, srcY1, dstX0, dstY0, dstX1, dstY1, mask, filter);
            };
            gl.glDeleteRenderbuffers = function (n, buffers) {
                for (var i = 0; i < n; i++) {
                    var id = getArray(buffers + i * 4, Uint32Array, 1)[0];
                    var buffer = GL.renderbuffers[id];

                    if (!buffer) continue;

                    gl.deleteRenderbuffer(buffer);
                    buffer.name = 0;
                    GL.renderbuffers[id] = null;
                }
            };
            let wbg = await init();
            miniquad_add_plugin({
                register_plugin: (a) => {
                    Object.assign(a.env, wbg);
                    Object.assign(a.env, gl);
                    a.wbg = wbg;
                },
                on_init: () => set_wasm(wasm_exports),
                version: "0.0.1",
                name: "wbg",
            });
            load("./${PROJECT_NAME}_bg.wasm");
        }
        window.run = function() {
            document.getElementById('container').removeAttribute('hidden');
            // document.getElementById('status').style.color = 'white';
            document.getElementById('glcanvas').removeAttribute('hidden');
            document.getElementById('glcanvas').style.background = 'black';
            document.getElementById('glcanvas').focus();
            full(); setTimeout(impl_run, 1);
        }
        window.on_game_start = function() {
            document.getElementById('status').parentNode.remove();
            // full();
        }
        window.full = function() {
            document.getElementById('container').requestFullscreen();
        }
    </script>
    <div id="run-container" style="display: flex; justify-content: center; align-items: center; height: 100%; flex-direction: column;">
        <p id="status">Game can't play audio unless a button has been clicked.</p>
        <button onclick="run()">Run Game</button>
    </div>
    <script>
        window.onload = () => {
            let old = XMLHttpRequest.prototype.open;
            let status = document.getElementById('status');
            XMLHttpRequest.prototype.open = function() {
                let url = arguments[1];
                status.innerText = 'Loading ' + url;
                old.call(this, ...arguments);
                this.onprogress = function(e) {
                    if (e.total) {
                        status.innerText = 'Loading ' + url + ' (' + Math.round(e.loaded / e.total * 100) + '%)';
                    } else {
                        status.innerText = 'Loading ' + url + ' (Loaded ' + (e.loaded / 1024).toFixed(1) + 'KB)';
                    }
                }
            }
        };
    </script>
</body>
</html>
END
)

# Build
cargo build --target wasm32-unknown-unknown --release --bin prpr-player

# Generate bindgen outputs
mkdir -p dist
wasm-bindgen target/wasm32-unknown-unknown/release/$PROJECT_NAME-player.wasm --out-dir dist --out-name prpr --target web --no-typescript

# Shim to tie the thing together
sed -i "s/import \* as __wbg_star0 from 'env';//" dist/$PROJECT_NAME.js
sed -i "s/let wasm;/let wasm; export const set_wasm = (w) => wasm = w;/" dist/$PROJECT_NAME.js
sed -i "s/imports\['env'\] = __wbg_star0;/return imports.wbg\;/" dist/$PROJECT_NAME.js
sed -i "s/const imports = getImports();/return getImports();/" dist/$PROJECT_NAME.js

# Create index from the HTML variable
echo "$HTML" > dist/index.html