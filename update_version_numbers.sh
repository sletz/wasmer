PREVIOUS_VERSION='0.4.2'
NEXT_VERSION='0.5.0'

# quick hack
fd Cargo.toml --exec sed -i '' "s/version = \"$PREVIOUS_VERSION\"/version = \"$NEXT_VERSION\"/"
echo "manually check changes to Cargo.toml"

# Order to upload packages in
## runtime-core
## win-exception-handler
## clif-backend
## llvm-backend
## singlepass-backend
## emscripten
## wasi
## runtime
## runtime-c-api
