FROM nginx:1.27-alpine

RUN apk add --no-cache openssl

COPY templates/default.conf.template /etc/nginx/templates/default.conf.template
COPY scripts/docker-entrypoint-extra.sh /docker-entrypoint.d/40-podcast-nginx.sh
RUN chmod +x /docker-entrypoint.d/40-podcast-nginx.sh
