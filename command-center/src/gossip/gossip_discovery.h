#pragma once

#include <QObject>
#include <QHash>
#include <QDateTime>
#include <QUdpSocket>
#include <QString>

struct WebServiceAnnouncement {
  QString serviceId;
  QString address;
  quint16 port = 0;
  QString basePath;
  QDateTime lastHeartbeat;
};

class GossipDiscovery : public QObject {
  Q_OBJECT
public:
  explicit GossipDiscovery(QObject *parent = nullptr);

  void start();
  QString currentServiceUrl() const;

signals:
  void serviceUrlChanged(const QString &url);
  void errorOccurred(const QString &message);

private:
  void handleGossip();
  void updateServiceUrl();
  QString buildServiceBaseUrl(const WebServiceAnnouncement &svc) const;

  QUdpSocket *gossipSocket = nullptr;
  QHash<QString, WebServiceAnnouncement> services;
  QString serviceBaseUrl;
};
