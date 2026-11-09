FROM rust:1-slim AS wasm
WORKDIR /engine
COPY engine/ .
RUN rustup target add wasm32-unknown-unknown \
 && cargo build --release --target wasm32-unknown-unknown --lib

FROM node:22-slim AS web
WORKDIR /app
COPY package*.json ./
RUN npm ci
COPY . .
COPY --from=wasm /engine/target/wasm32-unknown-unknown/release/engine.wasm src/sim/engine.wasm
RUN npx tsc --noEmit && npx vite build

FROM nginx:alpine
COPY nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=web /app/dist /usr/share/nginx/html
