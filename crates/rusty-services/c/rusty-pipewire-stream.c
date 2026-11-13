/*
 * Isolated PipeWire Video/Source worker for RustyOS XDG Desktop Portal ScreenCast (`RUSTYOS-057`).
 *
 * Reads length/dimension-framed RGBA frames from stdin (`RPW1`) and publishes them
 * to a PipeWire `Video/Source` stream so `rusty-compositor` stays free of `unsafe` FFI
 * and isolated from PipeWire thread-loop lifecycle/restarts.
 */

#define _POSIX_C_SOURCE 200809L

#include <errno.h>
#include <pthread.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#include <pipewire/pipewire.h>
#include <spa/buffer/meta.h>
#include <spa/param/buffers.h>
#include <spa/param/video/format-utils.h>
#include <spa/pod/builder.h>
#include <spa/utils/result.h>

#define RPW_MAGIC 0x52505731u /* 'RPW1' */
#define RPW_HEADER_BYTES 20u
#define RPW_MIN_DIM 16u
#define RPW_MAX_DIM 8192u
#define RPW_MAX_FRAME_BYTES (RPW_MAX_DIM * RPW_MAX_DIM * 4u)

static volatile sig_atomic_t g_quit = 0;

static void handle_signal(int sig) {
    (void)sig;
    g_quit = 1;
}

struct stream_app {
    struct pw_thread_loop *loop;
    struct pw_context *context;
    struct pw_core *core;
    struct spa_hook core_listener;
    struct pw_stream *stream;
    struct spa_hook stream_listener;

    uint32_t width;
    uint32_t height;
    uint32_t fps;
    enum spa_video_format negotiated_format;
    bool format_ready;
    bool ready_reported;
    bool ready_sync_pending;
    int ready_sync_seq;

    pthread_mutex_t frame_lock;
    uint8_t *front_buf;
    size_t front_cap;
    size_t front_size;
    uint32_t front_w;
    uint32_t front_h;
    uint64_t frame_seq;
};

static uint32_t read_le32(const uint8_t *p) {
    return (uint32_t)p[0]
        | ((uint32_t)p[1] << 8)
        | ((uint32_t)p[2] << 16)
        | ((uint32_t)p[3] << 24);
}

static bool read_exact(int fd, uint8_t *dst, size_t len) {
    size_t off = 0;
    while (off < len && !g_quit) {
        ssize_t n = read(fd, dst + off, len - off);
        if (n == 0) {
            return false;
        }
        if (n < 0) {
            if (errno == EINTR) {
                continue;
            }
            return false;
        }
        off += (size_t)n;
    }
    return off == len;
}

static uint64_t monotonic_nsec(void) {
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) {
        return 0;
    }
    return (uint64_t)ts.tv_sec * 1000000000ull + (uint64_t)ts.tv_nsec;
}

static const struct spa_pod *build_format_pod(
    struct spa_pod_builder *b,
    uint32_t width,
    uint32_t height,
    uint32_t fps
) {
    struct spa_rectangle size_def = SPA_RECTANGLE(width, height);
    struct spa_rectangle size_min = SPA_RECTANGLE(RPW_MIN_DIM, RPW_MIN_DIM);
    struct spa_rectangle size_max = SPA_RECTANGLE(RPW_MAX_DIM, RPW_MAX_DIM);
    struct spa_fraction fps_def = SPA_FRACTION(fps, 1);
    struct spa_fraction fps_min = SPA_FRACTION(1, 1);
    struct spa_fraction fps_max = SPA_FRACTION(240, 1);

    return (const struct spa_pod *)spa_pod_builder_add_object(
        b,
        SPA_TYPE_OBJECT_Format,
        SPA_PARAM_EnumFormat,
        SPA_FORMAT_mediaType,
        SPA_POD_Id(SPA_MEDIA_TYPE_video),
        SPA_FORMAT_mediaSubtype,
        SPA_POD_Id(SPA_MEDIA_SUBTYPE_raw),
        SPA_FORMAT_VIDEO_format,
        SPA_POD_CHOICE_ENUM_Id(
            5,
            SPA_VIDEO_FORMAT_BGRA,
            SPA_VIDEO_FORMAT_BGRA,
            SPA_VIDEO_FORMAT_BGRx,
            SPA_VIDEO_FORMAT_RGBA,
            SPA_VIDEO_FORMAT_RGBx
        ),
        SPA_FORMAT_VIDEO_size,
        SPA_POD_CHOICE_RANGE_Rectangle(&size_def, &size_min, &size_max),
        SPA_FORMAT_VIDEO_framerate,
        SPA_POD_CHOICE_RANGE_Fraction(&fps_def, &fps_min, &fps_max)
    );
}

static void fill_solid_rgba(uint8_t *buf, uint32_t w, uint32_t h) {
    size_t count = (size_t)w * (size_t)h;
    for (size_t i = 0; i < count; i++) {
        size_t o = i * 4u;
        buf[o + 0] = 15u;
        buf[o + 1] = 10u;
        buf[o + 2] = 25u;
        buf[o + 3] = 255u;
    }
}

static void report_ready_once(struct stream_app *app) {
    if (app->ready_reported || app->stream == NULL) {
        return;
    }
    uint32_t node_id = pw_stream_get_node_id(app->stream);
    if (node_id == SPA_ID_INVALID) {
        return;
    }
    uint64_t serial = 0;
    const struct pw_properties *props = pw_stream_get_properties(app->stream);
    if (props != NULL) {
        if (pw_properties_fetch_uint64(props, PW_KEY_OBJECT_SERIAL, &serial) < 0) {
            serial = (uint64_t)node_id;
        }
    }
    if (serial == 0) {
        serial = (uint64_t)node_id;
    }
    app->ready_reported = true;
    fprintf(stdout, "READY %u %llu\n", node_id, (unsigned long long)serial);
    fflush(stdout);
}

static void on_core_done(void *userdata, uint32_t id, int seq) {
    struct stream_app *app = (struct stream_app *)userdata;
    if (id == PW_ID_CORE && app->ready_sync_pending && seq == app->ready_sync_seq) {
        app->ready_sync_pending = false;
        report_ready_once(app);
    }
}

static const struct pw_core_events g_core_events = {
    PW_VERSION_CORE_EVENTS,
    .done = on_core_done,
};

static void on_state_changed(
    void *userdata,
    enum pw_stream_state old_state,
    enum pw_stream_state state,
    const char *error
) {
    (void)old_state;
    struct stream_app *app = (struct stream_app *)userdata;

    if (state == PW_STREAM_STATE_ERROR) {
        fprintf(stdout, "ERROR %s\n", error ? error : "pipewire stream error");
        fflush(stdout);
        g_quit = 1;
        return;
    }

    if (!app->ready_reported && !app->ready_sync_pending
        && (state == PW_STREAM_STATE_PAUSED || state == PW_STREAM_STATE_STREAMING)) {
        uint32_t node_id = pw_stream_get_node_id(app->stream);
        if (node_id != SPA_ID_INVALID) {
            app->ready_sync_seq = pw_core_sync(app->core, PW_ID_CORE, 0);
            if (app->ready_sync_seq >= 0) {
                app->ready_sync_pending = true;
            } else {
                report_ready_once(app);
            }
        }
    }

    if (app->ready_reported) {
        if (state == PW_STREAM_STATE_STREAMING) {
            fprintf(stdout, "STATE streaming\n");
            fflush(stdout);
        } else if (state == PW_STREAM_STATE_PAUSED) {
            fprintf(stdout, "STATE paused\n");
            fflush(stdout);
        }
    }
}

static void on_param_changed(void *userdata, uint32_t id, const struct spa_pod *param) {
    struct stream_app *app = (struct stream_app *)userdata;
    if (param == NULL || id != SPA_PARAM_Format) {
        return;
    }

    struct spa_video_info info;
    memset(&info, 0, sizeof(info));
    if (spa_format_video_parse(param, &info) < 0) {
        return;
    }
    if (info.media_type != SPA_MEDIA_TYPE_video || info.media_subtype != SPA_MEDIA_SUBTYPE_raw) {
        return;
    }

    uint32_t w = info.info.raw.size.width;
    uint32_t h = info.info.raw.size.height;
    if (w < RPW_MIN_DIM || w > RPW_MAX_DIM || h < RPW_MIN_DIM || h > RPW_MAX_DIM) {
        return;
    }

    app->negotiated_format = info.info.raw.format;
    app->width = w;
    app->height = h;
    app->format_ready = true;

    uint32_t stride = w * 4u;
    uint32_t size = stride * h;

    uint8_t pod_buf[1024];
    struct spa_pod_builder b = SPA_POD_BUILDER_INIT(pod_buf, sizeof(pod_buf));
    const struct spa_pod *params[2];
    params[0] = (const struct spa_pod *)spa_pod_builder_add_object(
        &b,
        SPA_TYPE_OBJECT_ParamBuffers,
        SPA_PARAM_Buffers,
        SPA_PARAM_BUFFERS_buffers,
        SPA_POD_CHOICE_RANGE_Int(4, 2, 8),
        SPA_PARAM_BUFFERS_blocks,
        SPA_POD_Int(1),
        SPA_PARAM_BUFFERS_size,
        SPA_POD_Int((int32_t)size),
        SPA_PARAM_BUFFERS_stride,
        SPA_POD_Int((int32_t)stride),
        SPA_PARAM_BUFFERS_dataType,
        SPA_POD_CHOICE_FLAGS_Int((1 << SPA_DATA_MemFd) | (1 << SPA_DATA_MemPtr))
    );
    params[1] = (const struct spa_pod *)spa_pod_builder_add_object(
        &b,
        SPA_TYPE_OBJECT_ParamMeta,
        SPA_PARAM_Meta,
        SPA_PARAM_META_type,
        SPA_POD_Id(SPA_META_Header),
        SPA_PARAM_META_size,
        SPA_POD_Int((int32_t)sizeof(struct spa_meta_header))
    );
    pw_stream_update_params(app->stream, params, 2);
}

static void on_process(void *userdata) {
    struct stream_app *app = (struct stream_app *)userdata;
    if (!app->format_ready) {
        return;
    }

    struct pw_buffer *b = pw_stream_dequeue_buffer(app->stream);
    if (b == NULL) {
        return;
    }

    struct spa_buffer *buf = b->buffer;
    if (buf == NULL || buf->n_datas < 1 || buf->datas[0].data == NULL) {
        pw_stream_queue_buffer(app->stream, b);
        return;
    }

    uint32_t dst_w = app->width;
    uint32_t dst_h = app->height;
    uint32_t dst_stride = dst_w * 4u;
    size_t max_bytes = (size_t)buf->datas[0].maxsize;
    size_t needed_bytes = (size_t)dst_stride * (size_t)dst_h;
    if (max_bytes < needed_bytes) {
        pw_stream_queue_buffer(app->stream, b);
        return;
    }

    pthread_mutex_lock(&app->frame_lock);
    uint8_t *dst = (uint8_t *)buf->datas[0].data;
    const bool want_bgra = (app->negotiated_format == SPA_VIDEO_FORMAT_BGRA
        || app->negotiated_format == SPA_VIDEO_FORMAT_BGRx);

    if (app->front_buf != NULL && app->front_w == dst_w && app->front_h == dst_h
        && app->front_size >= needed_bytes) {
        const uint8_t *src = app->front_buf;
        if (want_bgra) {
            size_t pixels = (size_t)dst_w * (size_t)dst_h;
            for (size_t i = 0; i < pixels; i++) {
                size_t o = i * 4u;
                dst[o + 0] = src[o + 2];
                dst[o + 1] = src[o + 1];
                dst[o + 2] = src[o + 0];
                dst[o + 3] = src[o + 3];
            }
        } else {
            memcpy(dst, src, needed_bytes);
        }
    } else if (app->front_buf != NULL && app->front_w > 0 && app->front_h > 0) {
        uint32_t copy_w = app->front_w < dst_w ? app->front_w : dst_w;
        uint32_t copy_h = app->front_h < dst_h ? app->front_h : dst_h;
        size_t src_stride = (size_t)app->front_w * 4u;
        memset(dst, 0, needed_bytes);
        for (uint32_t y = 0; y < copy_h; y++) {
            const uint8_t *src_row = app->front_buf + (size_t)y * src_stride;
            uint8_t *dst_row = dst + (size_t)y * (size_t)dst_stride;
            if (want_bgra) {
                for (uint32_t x = 0; x < copy_w; x++) {
                    size_t o = (size_t)x * 4u;
                    dst_row[o + 0] = src_row[o + 2];
                    dst_row[o + 1] = src_row[o + 1];
                    dst_row[o + 2] = src_row[o + 0];
                    dst_row[o + 3] = src_row[o + 3];
                }
            } else {
                memcpy(dst_row, src_row, (size_t)copy_w * 4u);
            }
        }
    } else {
        memset(dst, 0, needed_bytes);
    }
    uint64_t seq = ++app->frame_seq;
    pthread_mutex_unlock(&app->frame_lock);

    buf->datas[0].chunk->offset = 0;
    buf->datas[0].chunk->stride = (int32_t)dst_stride;
    buf->datas[0].chunk->size = (uint32_t)needed_bytes;

    struct spa_meta_header *hdr =
        (struct spa_meta_header *)spa_buffer_find_meta_data(buf, SPA_META_Header, sizeof(*hdr));
    if (hdr != NULL) {
        hdr->flags = 0;
        hdr->offset = 0;
        hdr->seq = seq;
        hdr->pts = (int64_t)monotonic_nsec();
        hdr->dts_offset = 0;
    }

    b->size = 1;
    pw_stream_queue_buffer(app->stream, b);
}

static const struct pw_stream_events g_stream_events = {
    PW_VERSION_STREAM_EVENTS,
    .state_changed = on_state_changed,
    .param_changed = on_param_changed,
    .process = on_process,
};

static bool parse_u32_arg(const char *s, uint32_t min_v, uint32_t max_v, uint32_t *out) {
    if (s == NULL || *s == '\0') {
        return false;
    }
    char *end = NULL;
    errno = 0;
    unsigned long v = strtoul(s, &end, 10);
    if (errno != 0 || end == s || *end != '\0' || v < min_v || v > max_v) {
        return false;
    }
    *out = (uint32_t)v;
    return true;
}

int main(int argc, char **argv) {
    uint32_t width = 1280;
    uint32_t height = 720;
    uint32_t fps = 60;
    const char *app_name = "RustyOS Desktop";
    const char *node_desc = "RustyOS ScreenCast";

    for (int i = 1; i + 1 < argc; i += 2) {
        if (strcmp(argv[i], "--width") == 0) {
            if (!parse_u32_arg(argv[i + 1], RPW_MIN_DIM, RPW_MAX_DIM, &width)) {
                fprintf(stderr, "invalid --width\n");
                return 2;
            }
        } else if (strcmp(argv[i], "--height") == 0) {
            if (!parse_u32_arg(argv[i + 1], RPW_MIN_DIM, RPW_MAX_DIM, &height)) {
                fprintf(stderr, "invalid --height\n");
                return 2;
            }
        } else if (strcmp(argv[i], "--fps") == 0) {
            if (!parse_u32_arg(argv[i + 1], 1, 240, &fps)) {
                fprintf(stderr, "invalid --fps\n");
                return 2;
            }
        } else if (strcmp(argv[i], "--app-name") == 0) {
            app_name = argv[i + 1];
        } else if (strcmp(argv[i], "--node-desc") == 0) {
            node_desc = argv[i + 1];
        } else {
            fprintf(stderr, "unknown option: %s\n", argv[i]);
            return 2;
        }
    }

    signal(SIGPIPE, SIG_IGN);
    struct sigaction sa;
    memset(&sa, 0, sizeof(sa));
    sa.sa_handler = handle_signal;
    sigemptyset(&sa.sa_mask);
    sigaction(SIGTERM, &sa, NULL);
    sigaction(SIGINT, &sa, NULL);

    pw_init(NULL, NULL);

    struct stream_app app;
    memset(&app, 0, sizeof(app));
    app.width = width;
    app.height = height;
    app.fps = fps;
    app.negotiated_format = SPA_VIDEO_FORMAT_BGRA;
    pthread_mutex_init(&app.frame_lock, NULL);

    size_t init_bytes = (size_t)width * (size_t)height * 4u;
    app.front_buf = (uint8_t *)malloc(init_bytes);
    if (app.front_buf == NULL) {
        fprintf(stderr, "OOM allocating initial frame\n");
        pw_deinit();
        return 1;
    }
    app.front_cap = init_bytes;
    app.front_size = init_bytes;
    app.front_w = width;
    app.front_h = height;
    fill_solid_rgba(app.front_buf, width, height);

    app.loop = pw_thread_loop_new("rustyos-screencast", NULL);
    if (app.loop == NULL) {
        fprintf(stderr, "pw_thread_loop_new failed\n");
        free(app.front_buf);
        pw_deinit();
        return 1;
    }

    app.context = pw_context_new(pw_thread_loop_get_loop(app.loop), NULL, 0);
    if (app.context == NULL) {
        fprintf(stderr, "pw_context_new failed\n");
        pw_thread_loop_destroy(app.loop);
        free(app.front_buf);
        pw_deinit();
        return 1;
    }

    pw_thread_loop_lock(app.loop);
    app.core = pw_context_connect(app.context, NULL, 0);
    if (app.core == NULL) {
        fprintf(stdout, "ERROR pipewire daemon unavailable\n");
        fflush(stdout);
        pw_thread_loop_unlock(app.loop);
        pw_context_destroy(app.context);
        pw_thread_loop_destroy(app.loop);
        free(app.front_buf);
        pw_deinit();
        return 1;
    }

    pw_core_add_listener(app.core, &app.core_listener, &g_core_events, &app);

    struct pw_properties *props = pw_properties_new(
        PW_KEY_MEDIA_TYPE, "Video",
        PW_KEY_MEDIA_CATEGORY, "Source",
        PW_KEY_MEDIA_ROLE, "Screen",
        PW_KEY_MEDIA_CLASS, "Video/Source",
        PW_KEY_APP_NAME, app_name,
        PW_KEY_NODE_NAME, "rustyos-screencast",
        PW_KEY_NODE_DESCRIPTION, node_desc,
        NULL
    );

    app.stream = pw_stream_new(app.core, "rustyos-screencast", props);
    if (app.stream == NULL) {
        fprintf(stdout, "ERROR pw_stream_new failed\n");
        fflush(stdout);
        pw_thread_loop_unlock(app.loop);
        pw_core_disconnect(app.core);
        pw_context_destroy(app.context);
        pw_thread_loop_destroy(app.loop);
        free(app.front_buf);
        pw_deinit();
        return 1;
    }

    pw_stream_add_listener(app.stream, &app.stream_listener, &g_stream_events, &app);

    uint8_t pod_buf[1024];
    struct spa_pod_builder b = SPA_POD_BUILDER_INIT(pod_buf, sizeof(pod_buf));
    const struct spa_pod *params[1];
    params[0] = build_format_pod(&b, width, height, fps);

    int conn_res = pw_stream_connect(
        app.stream,
        PW_DIRECTION_OUTPUT,
        PW_ID_ANY,
        (enum pw_stream_flags)(PW_STREAM_FLAG_DRIVER | PW_STREAM_FLAG_MAP_BUFFERS),
        params,
        1
    );
    if (conn_res < 0) {
        fprintf(stdout, "ERROR pw_stream_connect: %s\n", spa_strerror(conn_res));
        fflush(stdout);
        pw_thread_loop_unlock(app.loop);
        pw_stream_destroy(app.stream);
        pw_core_disconnect(app.core);
        pw_context_destroy(app.context);
        pw_thread_loop_destroy(app.loop);
        free(app.front_buf);
        pw_deinit();
        return 1;
    }

    pw_thread_loop_unlock(app.loop);
    if (pw_thread_loop_start(app.loop) < 0) {
        fprintf(stdout, "ERROR pw_thread_loop_start failed\n");
        fflush(stdout);
        pw_thread_loop_lock(app.loop);
        pw_stream_destroy(app.stream);
        pw_core_disconnect(app.core);
        pw_context_destroy(app.context);
        pw_thread_loop_unlock(app.loop);
        pw_thread_loop_destroy(app.loop);
        free(app.front_buf);
        pw_deinit();
        return 1;
    }

    uint8_t *staging = NULL;
    size_t staging_cap = 0;

    while (!g_quit) {
        uint8_t hdr[RPW_HEADER_BYTES];
        if (!read_exact(STDIN_FILENO, hdr, RPW_HEADER_BYTES)) {
            break;
        }
        uint32_t magic = read_le32(hdr + 0);
        uint32_t fw = read_le32(hdr + 4);
        uint32_t fh = read_le32(hdr + 8);
        uint32_t fstride = read_le32(hdr + 12);
        (void)read_le32(hdr + 16);

        if (magic != RPW_MAGIC || fw < RPW_MIN_DIM || fw > RPW_MAX_DIM
            || fh < RPW_MIN_DIM || fh > RPW_MAX_DIM || fstride != fw * 4u) {
            fprintf(stderr, "invalid frame packet header\n");
            break;
        }
        size_t payload_bytes = (size_t)fstride * (size_t)fh;
        if (payload_bytes > RPW_MAX_FRAME_BYTES) {
            break;
        }
        if (payload_bytes > staging_cap) {
            uint8_t *grown = (uint8_t *)realloc(staging, payload_bytes);
            if (grown == NULL) {
                break;
            }
            staging = grown;
            staging_cap = payload_bytes;
        }
        if (!read_exact(STDIN_FILENO, staging, payload_bytes)) {
            break;
        }

        bool size_changed = false;
        pthread_mutex_lock(&app.frame_lock);
        if (payload_bytes > app.front_cap) {
            uint8_t *grown = (uint8_t *)realloc(app.front_buf, payload_bytes);
            if (grown == NULL) {
                pthread_mutex_unlock(&app.frame_lock);
                break;
            }
            app.front_buf = grown;
            app.front_cap = payload_bytes;
        }
        memcpy(app.front_buf, staging, payload_bytes);
        app.front_size = payload_bytes;
        if (app.front_w != fw || app.front_h != fh) {
            size_changed = true;
        }
        app.front_w = fw;
        app.front_h = fh;
        pthread_mutex_unlock(&app.frame_lock);

        pw_thread_loop_lock(app.loop);
        if (size_changed && app.stream != NULL) {
            uint8_t upd_buf[1024];
            struct spa_pod_builder ub = SPA_POD_BUILDER_INIT(upd_buf, sizeof(upd_buf));
            const struct spa_pod *up_params[1];
            up_params[0] = build_format_pod(&ub, fw, fh, app.fps);
            pw_stream_update_params(app.stream, up_params, 1);
        }
        if (app.stream != NULL
            && pw_stream_get_state(app.stream, NULL) == PW_STREAM_STATE_STREAMING
            && pw_stream_is_driving(app.stream)) {
            pw_stream_trigger_process(app.stream);
        }
        pw_thread_loop_unlock(app.loop);
    }

    free(staging);

    pw_thread_loop_stop(app.loop);
    spa_hook_remove(&app.stream_listener);
    pw_stream_destroy(app.stream);
    spa_hook_remove(&app.core_listener);
    pw_core_disconnect(app.core);
    pw_context_destroy(app.context);
    pw_thread_loop_destroy(app.loop);
    pthread_mutex_destroy(&app.frame_lock);
    free(app.front_buf);
    pw_deinit();
    return 0;
}
