package main

import (
	"context"
	"family-stream-providers/internal/model"
	"family-stream-providers/internal/network"
	"family-stream-providers/internal/probe"
	"fmt"
	"net/url"
	"time"
)

// diagnoseContinuity validates consecutive media segments at startup to verify stream continuity.
// It reports missing or failing chunks without logging sensitive signed tokens or headers.
func diagnoseContinuity(
	ctx context.Context,
	client *network.Client,
	candidates []model.Candidate,
	continuityCount int,
	compareContext bool,
) {
	for _, candidate := range candidates {
		masterPlaylist, err := fetchDiagnosticPlaylist(ctx, client, candidate, candidate.URL)
		if err != nil {
			fmt.Printf("Continuity %s: master playlist failure=%s\n", candidate.Server, model.Category(err))
			continue
		}

		renditions := masterPlaylist.Metadata.Variants
		if len(renditions) == 0 {
			renditions = []model.Variant{{URL: candidate.URL}}
		}

		for _, rendition := range renditions {
			mediaPlaylist, err := fetchDiagnosticPlaylist(ctx, client, candidate, rendition.URL)
			if err != nil {
				fmt.Printf("Continuity %s: rendition playlist failure=%s\n", candidate.Server, model.Category(err))
				continue
			}

			resolutionHeight := 0
			if rendition.Height != nil {
				resolutionHeight = *rendition.Height
			}

			segmentsToInspect := min(continuityCount, len(mediaPlaylist.Samples))
			consecutiveSuccesses := 0

			for index, segment := range mediaPlaylist.Samples[:segmentsToInspect] {
				segmentURL, _ := url.Parse(segment.URL)
				sampleByteLimit := int64(32 * 1024)
				if segment.RangeLength > 0 {
					sampleByteLimit = min(sampleByteLimit, segment.RangeLength)
				}

				byteRangeHeader := fmt.Sprintf("bytes=%d-%d", segment.RangeStart, segment.RangeStart+sampleByteLimit-1)
				resp, err := client.Fetch(ctx, network.Request{
					URL:     segment.URL,
					Context: candidate.Context,
					Headers: map[string]string{
						"Range": byteRangeHeader,
					},
					Limit:  sampleByteLimit,
					Sample: true,
				})

				if err != nil || (resp.Status != 200 && resp.Status != 206) {
					fmt.Printf(
						"Continuity %s height=%d segment=%d host=%s status=%d failure=%s\n",
						candidate.Server,
						resolutionHeight,
						index,
						segmentURL.Hostname(),
						resp.Status,
						model.Category(err),
					)

					if compareContext && resp.Status == 403 {
						// Compare with fixed public VidCore headers used by the reference relay.
						// Never forward private cookies or session credentials to external CDNs.
						comparisonContext := model.FetchContext{
							Scopes: append([]model.FetchScope(nil), candidate.Context.Scopes...),
						}
						comparisonContext.Scopes = append(comparisonContext.Scopes, model.FetchScope{
							Origin: network.Origin(segmentURL),
							Headers: map[string]string{
								"Referer": "https://vidcore.io/",
								"Origin":  "https://vidcore.io",
							},
						})

						comparisonResp, comparisonErr := client.Fetch(ctx, network.Request{
							URL:     segment.URL,
							Context: comparisonContext,
							Headers: map[string]string{
								"Range": byteRangeHeader,
							},
							Limit:  sampleByteLimit,
							Sample: true,
						})

						fmt.Printf(
							"Public context comparison %s height=%d segment=%d status=%d fetch_ok=%t\n",
							candidate.Server,
							resolutionHeight,
							index,
							comparisonResp.Status,
							comparisonErr == nil,
						)
					}
					break
				}

				consecutiveSuccesses++
			}

			fmt.Printf(
				"Continuity %s height=%d startup_passed=%d requested=%d\n",
				candidate.Server,
				resolutionHeight,
				consecutiveSuccesses,
				segmentsToInspect,
			)
		}
	}
}

// fetchDiagnosticPlaylist downloads and parses an HLS manifest for diagnostic analysis.
func fetchDiagnosticPlaylist(
	ctx context.Context,
	client *network.Client,
	candidate model.Candidate,
	manifestURL string,
) (probe.Playlist, error) {
	fetchCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()

	resp, err := client.Fetch(fetchCtx, network.Request{
		URL:     manifestURL,
		Context: candidate.Context,
		Limit:   512 * 1024,
	})
	if err != nil {
		return probe.Playlist{}, err
	}

	if resp.Status != 200 {
		return probe.Playlist{}, model.Fail(model.UpstreamFailure, "diagnostic", "playlist rejected")
	}

	return probe.ParsePlaylist(resp.Body, resp.URL)
}
