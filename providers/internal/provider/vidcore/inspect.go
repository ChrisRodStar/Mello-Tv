package vidcore

import (
	"context"
	"family-stream-providers/internal/model"
	"family-stream-providers/internal/network"
	"fmt"
	"os"
	"path/filepath"
)

// CaptureReport summarizes the results of an offline capture run.
type CaptureReport struct {
	EmbedStatus   int
	ChunkCount    int
	MatchingFiles []string
}

// Capture downloads embed HTML and JavaScript chunks to a local directory for offline analysis.
// Downloaded script content is strictly saved as static text and is never executed or evaluated.
func Capture(ctx context.Context, client *network.Client, title model.Title, outputDir string) (CaptureReport, error) {
	report := CaptureReport{}

	path, err := embedPath(title)
	if err != nil {
		return report, err
	}

	fetchContext := model.FetchContext{
		Scopes: []model.FetchScope{
			{
				Origin: origin,
				Headers: map[string]string{
					"User-Agent":      userAgent,
					"Accept":          "*/*",
					"Accept-Language": "en-US,en;q=0.9",
				},
			},
		},
	}

	embedResp, err := client.Fetch(ctx, network.Request{
		URL:     origin + path,
		Context: fetchContext,
		Limit:   5_000_000,
	})
	if err != nil {
		return report, err
	}

	report.EmbedStatus = embedResp.Status

	embedFilePath := filepath.Join(outputDir, "embed.html")
	if err := os.WriteFile(embedFilePath, embedResp.Body, 0600); err != nil {
		return report, err
	}

	if embedResp.Status < 200 || embedResp.Status >= 300 || network.Origin(embedResp.URL) != network.Origin(mustOrigin()) {
		return report, model.Fail(model.UpstreamFailure, "embed", "capture response rejected")
	}

	parsedEmbed, err := ParseEmbed(string(embedResp.Body))
	if err != nil {
		return report, err
	}

	report.ChunkCount = len(parsedEmbed.Chunks)

	for i, chunkPath := range parsedEmbed.Chunks {
		chunkResp, err := client.Fetch(ctx, network.Request{
			URL:     origin + chunkPath,
			Context: fetchContext,
			Limit:   2_500_000,
		})
		if err != nil {
			return report, err
		}

		if chunkResp.Status < 200 || chunkResp.Status >= 300 || network.Origin(chunkResp.URL) != network.Origin(mustOrigin()) {
			continue
		}

		fileName := fmt.Sprintf("chunk-%02d.js", i)
		if err := os.WriteFile(filepath.Join(outputDir, fileName), chunkResp.Body, 0600); err != nil {
			return report, err
		}

		if _, err := ParseMaterial(string(chunkResp.Body)); err == nil {
			report.MatchingFiles = append(report.MatchingFiles, fileName)
		}
	}

	return report, nil
}
