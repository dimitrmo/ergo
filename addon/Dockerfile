# The binary and UI are built outside (make addon), so the image build on the
# Pi only copies files and takes seconds.
FROM alpine:3.24

ARG BUILD_ARCH=amd64

RUN apk add --no-cache nginx \
    && rm -f /etc/nginx/http.d/default.conf

COPY rootfs /
COPY ui /usr/share/ergo/ui
COPY bin/ergo-${BUILD_ARCH} /usr/bin/ergo

ENV ERGO_DATA_DIR=/data

# Liveness goes through nginx to the backend, so a dead nginx or a dead
# backend both mark the container unhealthy and get the add-on restarted.
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 \
    CMD wget -qO- http://127.0.0.1:8099/health || exit 1

CMD ["/usr/bin/run.sh"]
