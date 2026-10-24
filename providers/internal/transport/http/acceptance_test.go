package httptransport

// Opt-in fixture: production HTTP/coordinator, synthetic adapters and probing.
import (
	"context"
	"encoding/json"
	"net"
	"os"
	"os/signal"
	"syscall"
	"testing"
	"time"

	"stv-providers/internal/model"
	"stv-providers/internal/provider"
	"stv-providers/internal/search"
)

type fixtureAdapter struct {
	id model.ProviderID
}

func (a fixtureAdapter) ID() model.ProviderID {
	return a.id
}
func (a fixtureAdapter) Capabilities() provider.Capabilities {
	return provider.Capabilities{}
}
func (a fixtureAdapter) Resolve(_ context.Context, lookup model.Lookup) (model.AdapterResult, error) {
	format, path := model.HLS, "/master.m3u8"
	if lookup.Device.Formats[0] == model.MP4 {
		format, path = model.MP4, "/movie.mp4"
	}
	source := model.Candidate{
		ID:       "synthetic-source",
		Provider: a.id,
		Title:    lookup.Title,

		URL:    "http://source.example.com" + path,
		Format: format,

		Context: model.FetchContext{Scopes: []model.FetchScope{{
			Origin:     "http://source.example.com",
			PathPrefix: "/",
			Headers:    map[string]string{"Authorization": "synthetic-upstream"},
		}}},
	}
	if format == model.HLS {
		source.SelectedVariantURL = "http://source.example.com/v.m3u8"
	}
	return model.AdapterResult{Candidates: []model.Candidate{source}}, nil
}

type fixtureProber struct {
}

func (fixtureProber) ProbeStream(_ context.Context, sources <-chan model.Candidate) []model.Probed {
	var results []model.Probed
	rate := 24_000_000.0
	for source := range sources {
		results = append(results, model.Probed{
			Source: source,
			Observation: model.Observation{
				At:              time.Now(),
				Accessible:      true,
				Format:          source.Format,
				ProtectionKnown: true,
				DeliveryBPS:     &rate,
			},
		})
	}
	return results
}

type fixtureEngine struct {
	*search.Coordinator
}

func (fixtureEngine) Check(context.Context) error {
	return nil
}
func TestAcceptanceHTTPService(t *testing.T) {
	readyPath := os.Getenv("FAMILY_STREAM_SYNTHETIC_READY")
	if readyPath == "" {
		t.Skip("explicit cross-language fixture only")
	}
	var adapters []provider.Adapter
	for _, id := range model.PlannedProviders {
		adapters = append(adapters, fixtureAdapter{id: id})
	}
	registry, err := provider.NewRegistry(adapters...)
	if err != nil {
		t.Fatal(err)
	}
	coordinator, err := search.New(registry, fixtureProber{}, search.DefaultLimits(), nil)
	if err != nil {
		t.Fatal(err)
	}
	defer coordinator.Close()
	handler, err := New(fixtureEngine{coordinator}, []byte(testBearer))
	if err != nil {
		t.Fatal(err)
	}
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	ready, _ := json.Marshal(map[string]int{"port": listener.Addr().(*net.TCPAddr).Port})
	if os.WriteFile(readyPath, ready, 0600) != nil {
		t.Fatal("write readiness")
	}
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	ctx, deadline := context.WithTimeout(ctx, 45*time.Second)
	defer deadline()
	if err := Serve(ctx, listener, handler); err != nil {
		t.Fatal(err)
	}
}
