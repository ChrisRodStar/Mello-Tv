# HTTP behavior cases

Implement synthetic peers for exact 206 and Content-Range totals, suffix/open-ended
ranges, malformed/multipart ranges, valid 416, HEAD rejection with bounded range
discovery, ignored unconditional Range, If-Range full-200 behavior, truncated and
overlong bodies, HTML masquerading as media, nonzero MP4 seek offsets, redirect
credential changes, slow body/reader cancellation and compressed response rejection.
Assertions must observe bytes and resource/task limits, not mirror helper internals.
No upstream media captures belong here.
