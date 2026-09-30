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

CMD ["/usr/bin/run.sh"]
