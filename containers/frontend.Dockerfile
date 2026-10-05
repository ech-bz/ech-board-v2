FROM docker.io/library/node:22-alpine AS build

WORKDIR /src
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY frontend/ .
ARG VITE_RELAY_URL=http://localhost:8080
ARG VITE_BASE_PATH=/
ENV VITE_RELAY_URL=$VITE_RELAY_URL
ENV VITE_BASE_PATH=$VITE_BASE_PATH
RUN npm run build

FROM docker.io/library/nginx:alpine
COPY --from=build /src/dist /usr/share/nginx/html
COPY containers/nginx.conf /etc/nginx/conf.d/default.conf
