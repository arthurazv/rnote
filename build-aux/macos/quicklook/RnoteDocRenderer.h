#import <AppKit/AppKit.h>
#import <CoreGraphics/CoreGraphics.h>
#import <zlib.h>

static inline NSData *RnoteDecompressGzip(NSData *compressedData) {
    if ([compressedData length] == 0) return nil;
    z_stream strm;
    memset(&strm, 0, sizeof(strm));
    if (inflateInit2(&strm, 16 + MAX_WBITS) != Z_OK) return nil;
    strm.next_in = (Bytef *)[compressedData bytes];
    strm.avail_in = (uInt)[compressedData length];

    NSMutableData *decompressed = [NSMutableData dataWithCapacity:[compressedData length] * 3];
    Bytef buffer[65536];
    int status;
    do {
        strm.next_out = buffer;
        strm.avail_out = sizeof(buffer);
        status = inflate(&strm, Z_SYNC_FLUSH);
        if (status == Z_OK || status == Z_STREAM_END) {
            [decompressed appendBytes:buffer length:sizeof(buffer) - strm.avail_out];
        }
    } while (status == Z_OK);
    inflateEnd(&strm);
    return decompressed;
}

static inline NSImage *RnoteRenderDocument(NSString *path, CGSize targetSize) {
    NSData *comp = [NSData dataWithContentsOfFile:path];
    if (!comp) return nil;
    NSData *decomp = RnoteDecompressGzip(comp);
    if (!decomp) return nil;

    NSDictionary *json = [NSJSONSerialization JSONObjectWithData:decomp options:0 error:nil];
    if (!json) return nil;

    // Check if embedded thumbnail_png is present
    NSString *embeddedB64 = json[@"thumbnail_png"];
    if (embeddedB64) {
        NSData *png = [[NSData alloc] initWithBase64EncodedString:embeddedB64 options:NSDataBase64DecodingIgnoreUnknownCharacters];
        if (png) return [[NSImage alloc] initWithData:png];
    }

    NSDictionary *snap = json[@"data"][@"engine_snapshot"];
    if (!snap) snap = json[@"engine_snapshot"];
    if (!snap) return nil;

    NSDictionary *doc = snap[@"document"];
    double docW = [doc[@"width"] doubleValue];
    double docH = [doc[@"height"] doubleValue];
    if (docW <= 0) docW = 794;
    if (docH <= 0) docH = 1123;

    double scale = 1.0;
    if (targetSize.width > 0 && targetSize.height > 0) {
        double sx = targetSize.width / docW;
        double sy = targetSize.height / docH;
        scale = MIN(sx, sy);
    }
    int outW = (int)(docW * scale);
    int outH = (int)(docH * scale);
    if (outW <= 0) outW = 512;
    if (outH <= 0) outH = 512;

    NSBitmapImageRep *rep = [[NSBitmapImageRep alloc]
        initWithBitmapDataPlanes:NULL
                      pixelsWide:outW
                      pixelsHigh:outH
                   bitsPerSample:8
                 samplesPerPixel:4
                        hasAlpha:YES
                        isPlanar:NO
                  colorSpaceName:NSCalibratedRGBColorSpace
                     bytesPerRow:0
                    bitsPerPixel:0];

    NSGraphicsContext *gc = [NSGraphicsContext graphicsContextWithBitmapImageRep:rep];
    [NSGraphicsContext saveGraphicsState];
    [NSGraphicsContext setCurrentContext:gc];
    CGContextRef ctx = [gc CGContext];

    // Background color
    NSDictionary *bg = doc[@"background"];
    NSDictionary *bgCol = bg[@"color"];
    if (bgCol) {
        double r = [bgCol[@"r"] doubleValue];
        double g = [bgCol[@"g"] doubleValue];
        double b = [bgCol[@"b"] doubleValue];
        double a = bgCol[@"a"] ? [bgCol[@"a"] doubleValue] : 1.0;
        CGContextSetRGBFillColor(ctx, r, g, b, a);
    } else {
        CGContextSetRGBFillColor(ctx, 0.12, 0.12, 0.14, 1.0);
    }
    CGContextFillRect(ctx, CGRectMake(0, 0, outW, outH));

    // CoreGraphics origin is bottom-left, Rnote is top-left
    CGContextTranslateCTM(ctx, 0, outH);
    CGContextScaleCTM(ctx, scale, -scale);

    // Draw strokes
    NSArray *strokes = snap[@"stroke_components"];
    for (NSDictionary *item in strokes) {
        if (![item isKindOfClass:[NSDictionary class]]) continue;
        NSDictionary *val = item[@"value"];
        if (![val isKindOfClass:[NSDictionary class]]) continue;

        NSDictionary *bs = val[@"brushstroke"];
        if (bs) {
            NSDictionary *path = bs[@"path"];
            NSDictionary *style = bs[@"style"];
            NSDictionary *smooth = style[@"smooth"] ?: style[@"textured"];
            NSDictionary *col = smooth[@"stroke_color"];
            double width = [smooth[@"stroke_width"] doubleValue];
            if (width <= 0) width = 2.0;

            if (col) {
                double r = [col[@"r"] doubleValue];
                double g = [col[@"g"] doubleValue];
                double b = [col[@"b"] doubleValue];
                double a = col[@"a"] ? [col[@"a"] doubleValue] : 1.0;
                CGContextSetRGBStrokeColor(ctx, r, g, b, a);
            } else {
                CGContextSetRGBStrokeColor(ctx, 1.0, 1.0, 1.0, 1.0);
            }
            CGContextSetLineWidth(ctx, width);
            CGContextSetLineCap(ctx, kCGLineCapRound);
            CGContextSetLineJoin(ctx, kCGLineJoinRound);

            NSDictionary *start = path[@"start"];
            if (start) {
                double sx = [start[@"x"] doubleValue];
                double sy = [start[@"y"] doubleValue];
                CGContextBeginPath(ctx);
                CGContextMoveToPoint(ctx, sx, sy);

                NSArray *segments = path[@"segments"];
                for (NSDictionary *seg in segments) {
                    NSDictionary *pt = seg[@"point"] ?: seg[@"end"] ?: seg;
                    if (pt[@"x"] && pt[@"y"]) {
                        CGContextAddLineToPoint(ctx, [pt[@"x"] doubleValue], [pt[@"y"] doubleValue]);
                    } else if (seg[@"line_to"]) {
                        NSDictionary *lp = seg[@"line_to"];
                        CGContextAddLineToPoint(ctx, [lp[@"x"] doubleValue], [lp[@"y"] doubleValue]);
                    } else if (seg[@"quad_to"]) {
                        NSDictionary *ctrl = seg[@"quad_to"][@"ctrl"];
                        NSDictionary *to = seg[@"quad_to"][@"to"];
                        CGContextAddQuadCurveToPoint(ctx, [ctrl[@"x"] doubleValue], [ctrl[@"y"] doubleValue], [to[@"x"] doubleValue], [to[@"y"] doubleValue]);
                    } else if (seg[@"curve_to"]) {
                        NSDictionary *c1 = seg[@"curve_to"][@"ctrl1"];
                        NSDictionary *c2 = seg[@"curve_to"][@"ctrl2"];
                        NSDictionary *to = seg[@"curve_to"][@"to"];
                        CGContextAddCurveToPoint(ctx, [c1[@"x"] doubleValue], [c1[@"y"] doubleValue], [c2[@"x"] doubleValue], [c2[@"y"] doubleValue], [to[@"x"] doubleValue], [to[@"y"] doubleValue]);
                    }
                }
                CGContextStrokePath(ctx);
            }
        }
    }

    [NSGraphicsContext restoreGraphicsState];

    NSImage *image = [[NSImage alloc] initWithSize:NSMakeSize(outW, outH)];
    [image addRepresentation:rep];
    return image;
}
