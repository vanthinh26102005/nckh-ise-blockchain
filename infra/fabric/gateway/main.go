package main

import (
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/hyperledger/fabric-gateway/pkg/client"
	"github.com/hyperledger/fabric-gateway/pkg/hash"
	"github.com/hyperledger/fabric-gateway/pkg/identity"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials"
)

type config struct {
	MSPID        string
	CertDir      string
	KeyDir       string
	TLSCertPath  string
	PeerEndpoint string
	GatewayPeer  string
	Channel      string
	Chaincode    string
	Listen       string
}

type server struct {
	contract *client.Contract
}

type eventRequest struct {
	CanonicalEvent string `json:"canonicalEvent"`
	Digest         string `json:"digest"`
}

func main() {
	cfg, err := loadConfig()
	if err != nil {
		panic(err)
	}
	connection, gateway, err := connect(cfg)
	if err != nil {
		panic(err)
	}
	defer connection.Close()
	defer gateway.Close()

	s := server{contract: gateway.GetNetwork(cfg.Channel).GetContract(cfg.Chaincode)}
	mux := http.NewServeMux()
	mux.HandleFunc("GET /health", s.health)
	mux.HandleFunc("POST /events", s.createEvent)
	mux.HandleFunc("GET /events/", s.readEvent)
	httpServer := http.Server{Addr: cfg.Listen, Handler: mux, ReadHeaderTimeout: 10 * time.Second}
	fmt.Printf("e2epcis gateway listening on %s\n", cfg.Listen)
	if err := httpServer.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
		panic(err)
	}
}

func loadConfig() (config, error) {
	cryptoRoot := os.Getenv("FABRIC_CRYPTO_ROOT")
	if cryptoRoot == "" {
		return config{}, errors.New("FABRIC_CRYPTO_ROOT is required")
	}
	return config{
		MSPID:        envOr("FABRIC_MSP_ID", "Org1MSP"),
		CertDir:      filepath.Join(cryptoRoot, "users/User1@org1.example.com/msp/signcerts"),
		KeyDir:       filepath.Join(cryptoRoot, "users/User1@org1.example.com/msp/keystore"),
		TLSCertPath:  filepath.Join(cryptoRoot, "peers/peer0.org1.example.com/tls/ca.crt"),
		PeerEndpoint: envOr("FABRIC_PEER_ENDPOINT", "dns:///localhost:7051"),
		GatewayPeer:  envOr("FABRIC_GATEWAY_PEER", "peer0.org1.example.com"),
		Channel:      envOr("FABRIC_CHANNEL", "mychannel"),
		Chaincode:    envOr("FABRIC_CHAINCODE", "e2epcis"),
		Listen:       envOr("GATEWAY_LISTEN", ":8080"),
	}, nil
}

func connect(cfg config) (*grpc.ClientConn, *client.Gateway, error) {
	tlsPEM, err := os.ReadFile(cfg.TLSCertPath)
	if err != nil {
		return nil, nil, fmt.Errorf("read TLS certificate: %w", err)
	}
	tlsCertificate, err := identity.CertificateFromPEM(tlsPEM)
	if err != nil {
		return nil, nil, fmt.Errorf("parse TLS certificate: %w", err)
	}
	pool := x509.NewCertPool()
	pool.AddCert(tlsCertificate)
	connection, err := grpc.NewClient(
		cfg.PeerEndpoint,
		grpc.WithTransportCredentials(credentials.NewClientTLSFromCert(pool, cfg.GatewayPeer)),
	)
	if err != nil {
		return nil, nil, fmt.Errorf("connect to Fabric peer: %w", err)
	}
	certificatePEM, err := readFirstFile(cfg.CertDir)
	if err != nil {
		connection.Close()
		return nil, nil, fmt.Errorf("read gateway certificate: %w", err)
	}
	certificate, err := identity.CertificateFromPEM(certificatePEM)
	if err != nil {
		connection.Close()
		return nil, nil, fmt.Errorf("parse gateway certificate: %w", err)
	}
	clientIdentity, err := identity.NewX509Identity(cfg.MSPID, certificate)
	if err != nil {
		connection.Close()
		return nil, nil, fmt.Errorf("create gateway identity: %w", err)
	}
	privateKeyPEM, err := readFirstFile(cfg.KeyDir)
	if err != nil {
		connection.Close()
		return nil, nil, fmt.Errorf("read gateway private key: %w", err)
	}
	privateKey, err := identity.PrivateKeyFromPEM(privateKeyPEM)
	if err != nil {
		connection.Close()
		return nil, nil, fmt.Errorf("parse gateway private key: %w", err)
	}
	sign, err := identity.NewPrivateKeySign(privateKey)
	if err != nil {
		connection.Close()
		return nil, nil, fmt.Errorf("create gateway signer: %w", err)
	}
	gateway, err := client.Connect(
		clientIdentity,
		client.WithSign(sign),
		client.WithHash(hash.SHA256),
		client.WithClientConnection(connection),
		client.WithEvaluateTimeout(15*time.Second),
		client.WithEndorseTimeout(30*time.Second),
		client.WithSubmitTimeout(30*time.Second),
		client.WithCommitStatusTimeout(time.Minute),
	)
	if err != nil {
		connection.Close()
		return nil, nil, fmt.Errorf("create Fabric gateway: %w", err)
	}
	return connection, gateway, nil
}

func (s server) health(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{"status": "ok"})
}

func (s server) createEvent(w http.ResponseWriter, r *http.Request) {
	defer r.Body.Close()
	var request eventRequest
	if err := json.NewDecoder(io.LimitReader(r.Body, 8<<10)).Decode(&request); err != nil {
		writeError(w, http.StatusBadRequest, err)
		return
	}
	event, err := base64.StdEncoding.DecodeString(request.CanonicalEvent)
	if err != nil || len(event) != 86 || event[0] != 1 {
		writeError(w, http.StatusBadRequest, errors.New("canonicalEvent must be EPCIS Event V1 base64"))
		return
	}
	digest, err := hex.DecodeString(strings.TrimPrefix(request.Digest, "0x"))
	calculated := sha256.Sum256(event)
	if err != nil || len(digest) != sha256.Size || string(digest) != string(calculated[:]) {
		writeError(w, http.StatusBadRequest, errors.New("digest must be SHA-256(canonicalEvent)"))
		return
	}
	result, err := s.contract.Submit(
		"CreateEvent",
		client.WithArguments(
			request.CanonicalEvent,
			strings.TrimPrefix(request.Digest, "0x"),
		),
		client.WithEndorsingOrganizations("Org1MSP", "Org2MSP"),
	)
	if err != nil {
		writeError(w, http.StatusConflict, err)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusCreated)
	_, _ = w.Write(result)
}

func (s server) readEvent(w http.ResponseWriter, r *http.Request) {
	eventID := strings.TrimPrefix(r.URL.Path, "/events/")
	if eventID == "" || strings.Contains(eventID, "/") {
		writeError(w, http.StatusBadRequest, errors.New("event ID is required"))
		return
	}
	result, err := s.contract.EvaluateTransaction("ReadEvent", eventID)
	if err != nil {
		writeError(w, http.StatusNotFound, err)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	_, _ = w.Write(result)
}

func readFirstFile(directory string) ([]byte, error) {
	entries, err := os.ReadDir(directory)
	if err != nil {
		return nil, err
	}
	for _, entry := range entries {
		if !entry.IsDir() {
			return os.ReadFile(filepath.Join(directory, entry.Name()))
		}
	}
	return nil, fmt.Errorf("no file in %s", directory)
}

func writeJSON(w http.ResponseWriter, status int, value any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(value)
}

func writeError(w http.ResponseWriter, status int, err error) {
	writeJSON(w, status, map[string]string{"error": err.Error()})
}

func envOr(key, fallback string) string {
	if value := os.Getenv(key); value != "" {
		return value
	}
	return fallback
}
